// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{borrow::Cow, collections::HashMap, str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use log::{debug, trace};
use regex::Regex;
use rust_embed::Embed;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, Unexpected},
};
use serde_json::json;
use tokio::{net::TcpListener, sync::Mutex};
use translation_memory::TranslationStore;
use unic_langid::LanguageIdentifier;

#[derive(Embed)]
#[folder = "src/web_server"]
struct Assets;

pub async fn web_server(store: Arc<Mutex<TranslationStore>>) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route("/style.css", get(style))
        .route("/script.js", get(script))
        .route("/favicon.ico", get(language_icon))
        .route("/icon/language.svg", get(language_icon))
        .route("/icon/loading.svg", get(loading_icon))
        .route("/icon/remove.svg", get(remove_icon))
        .route("/icon/search.svg", get(search_icon))
        .route("/metadata", get(metadata_api))
        .route("/query", get(query_api))
        .with_state(store);

    let listener = TcpListener::bind("127.0.0.1:2013").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

macro_rules! static_page {
    ($name:ident, $file_path:literal, $content_type:literal) => {
        async fn $name() -> Response {
            debug!(concat!("Request for '", $file_path, "'"));
            let content_type_header = (
                header::CONTENT_TYPE,
                HeaderValue::from_static($content_type),
            );
            Assets::get($file_path)
                .map(|file| ([content_type_header], file.data.to_vec()))
                .ok_or(())
                .into_response()
        }
    };
}

static_page!(index, "index.html", "text/html");
static_page!(style, "style.css", "text/css");
static_page!(script, "script.js", "text/javascript");
static_page!(language_icon, "language.svg", "image/svg+xml");
static_page!(loading_icon, "loading.svg", "image/svg+xml");
static_page!(remove_icon, "remove.svg", "image/svg+xml");
static_page!(search_icon, "search.svg", "image/svg+xml");

#[derive(Deserialize, Serialize)]
struct QueryParams {
    search: Option<String>,

    #[serde(deserialize_with = "deserialize_languages")]
    languages: Vec<LanguageIdentifier>,

    #[serde(deserialize_with = "deserialize_scopes")]
    scopes: Vec<String>,

    limit: Option<usize>,
    skip: Option<usize>,

    count: Option<bool>,
}

fn deserialize_languages<'de, D>(deserializer: D) -> Result<Vec<LanguageIdentifier>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: String = String::deserialize(deserializer)?;
    let mut list = Vec::new();
    for value in value.split(',') {
        list.push(
            LanguageIdentifier::from_str(value)
                .map_err(|_| de::Error::invalid_value(Unexpected::Str(value), &"a language ID"))?,
        )
    }
    Ok(list)
}

fn deserialize_scopes<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: String = String::deserialize(deserializer)?;
    Ok(value.split(',').map(|v| v.to_string()).collect())
}

async fn query_api(
    State(store): State<Arc<Mutex<TranslationStore>>>,
    Query(params): Query<QueryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    debug!(
        "Request for '/query':\
      \n{{\
      \n    search: {},\
      \n    languages: \"{}\", scopes: {},\
      \n    limit: {}, skip: {}, count: {}\
      \n}}",
        params
            .search
            .as_ref()
            .map(|v| Cow::Owned(format!("\"{v}\"")))
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .languages
            .iter()
            .map(|lang_id| lang_id.to_string())
            .reduce(|acc, lang| acc + "," + &lang)
            .unwrap_or_default(),
        if params.scopes.len() > 3 {
            format!("<{}>", params.scopes.len())
        } else {
            format!("\"{}\"", params.scopes.join(","))
        },
        params
            .limit
            .map(|limit| Cow::Owned(limit.to_string()))
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .skip
            .map(|skip| Cow::Owned(skip.to_string()))
            .unwrap_or(Cow::Borrowed("undefined")),
        match params.count {
            Some(true) => "true",
            Some(false) => "false",
            None => "undefined",
        },
    );

    let search_filters = if let Some(search) = &params.search {
        parse_search(search).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    } else {
        vec![]
    };

    let store = store.lock().await;
    let translations = store
        .translations()
        .filter(|(scope, lang_id, translation)| {
            if !params.languages.contains(lang_id) {
                return false;
            }
            if !params.scopes.contains(scope) {
                return false;
            }

            for (filter, filter_mode) in &search_filters {
                let matches = match filter {
                    SearchFilter::OriginalRegex(regex) => regex.is_match(&translation.original),
                    SearchFilter::TranslationRegex(regex) => {
                        regex.is_match(&translation.translation)
                    }
                    SearchFilter::EitherRegex(regex) => {
                        regex.is_match(&translation.original)
                            || regex.is_match(&translation.translation)
                    }
                    SearchFilter::Scope(s) => {
                        if s == *scope {
                            true
                        } else if let Some(provider) = store.provider(scope) {
                            provider.name() == s || provider.group_name() == Some(s)
                        } else {
                            false
                        }
                    }
                    SearchFilter::Language(l) => l == *lang_id,
                };
                match (filter_mode, matches) {
                    (SearchFilterMode::Require, true) => {}
                    (SearchFilterMode::Require, false) => return false,
                    (SearchFilterMode::Block, true) => return false,
                    (SearchFilterMode::Block, false) => {}
                }
            }

            true
        });

    if params.count.unwrap_or(false) {
        let count = translations.count();

        trace!("Request for '/query': returning {count}",);

        return Ok(Json(serde_json::to_value(count).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?));
    }

    let translations = translations.map(|(scope, lang_id, translation)| {
        fn regex_parse(regex: Option<&Regex>, string: &str) -> Vec<serde_json::Value> {
            if let Some(regex) = regex {
                let mut original = Vec::new();
                let mut prev = 0;
                for capture in regex.find_iter(string) {
                    original.push(json!(string[prev..capture.start()]));
                    original.push(json!({
                        "marked": true,
                        "text": string[capture.start()..capture.end()],
                    }));
                    prev = capture.end();
                }
                original.push(json!(string[prev..]));
                original
            } else {
                vec![json!(string)]
            }
        }

        let original_regex = search_filters
            .iter()
            .filter_map(|filter| match filter {
                (
                    SearchFilter::OriginalRegex(regex) | SearchFilter::EitherRegex(regex),
                    SearchFilterMode::Require,
                ) => Some(regex),
                _ => None,
            })
            .find(|regex| regex.is_match(&translation.original));
        let translation_regex = search_filters
            .iter()
            .filter_map(|filter| match filter {
                (
                    SearchFilter::TranslationRegex(regex) | SearchFilter::EitherRegex(regex),
                    SearchFilterMode::Require,
                ) => Some(regex),
                _ => None,
            })
            .find(|regex| regex.is_match(&translation.translation));

        let mut json = json!({
            "scope": scope,
            "language": lang_id,
            "original": regex_parse(original_regex, &translation.original),
            "translation": regex_parse(translation_regex, &translation.translation),
            "source": translation.source,
        });
        if let Some(comment) = &translation.comment {
            json["comment"] = json!(comment);
        }
        if let Some(key) = &translation.key {
            json["key"] = json!(key);
        }
        json
    });

    let translations: Vec<serde_json::Value> = match (params.limit, params.skip) {
        (Some(limit), Some(skip)) => translations.skip(skip).take(limit).collect(),
        (Some(limit), None) => translations.take(limit).collect(),
        (None, Some(skip)) => translations.skip(skip).collect(),
        (None, None) => translations.collect(),
    };

    trace!(
        "Request for '/query': returning {} translations",
        translations.len(),
    );

    Ok(Json(serde_json::to_value(translations).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

enum SearchFilter {
    /// Check if either `original` or `translation` matches the regex.
    EitherRegex(Regex),
    /// Check if `original` matches the regex.
    OriginalRegex(Regex),
    /// Check if `translation` matches the regex.
    TranslationRegex(Regex),
    /// Check if translation is from the provider with the specified id.
    Scope(String),
    /// Check if translation is translated to the specified language.
    Language(LanguageIdentifier),
}

enum SearchFilterMode {
    /// Require a search filter to be `true` to include translation in the results.
    Require,
    /// Require a search filter to be `false` to include translation in the results.
    Block,
}

fn parse_search(search: &str) -> anyhow::Result<Vec<(SearchFilter, SearchFilterMode)>> {
    if !search.contains(':') {
        if search.is_empty() {
            return Ok(vec![]);
        } else {
            return Ok(vec![(
                SearchFilter::EitherRegex(Regex::new(&format!("(?i){search}"))?),
                SearchFilterMode::Require,
            )]);
        }
    }

    let mut search_filters = Vec::new();

    let mut search_rest = String::new();
    let mut first = true;
    for part in split_search(search) {
        if let Some((key, value)) = part.split_once(':') {
            let (key, mode) = if let Some(base_key) = key.strip_prefix('-') {
                (base_key, SearchFilterMode::Block)
            } else {
                (key, SearchFilterMode::Require)
            };

            match key {
                "o" | "original" => {
                    search_filters.push((
                        SearchFilter::OriginalRegex(Regex::new(&format!("(?i){value}"))?),
                        mode,
                    ));
                    continue;
                }
                "t" | "translation" => {
                    search_filters.push((
                        SearchFilter::TranslationRegex(Regex::new(&format!("(?i){value}"))?),
                        mode,
                    ));
                    continue;
                }
                "s" | "scope" => {
                    search_filters.push((SearchFilter::Scope(value.to_string()), mode));
                    continue;
                }
                "l" | "lang" | "language" => {
                    search_filters.push((SearchFilter::Language(value.parse()?), mode));
                    continue;
                }
                _ => {}
            }
        }

        if first {
            first = false;
        } else {
            search_rest.push(' ');
        }
        search_rest.push_str(&part)
    }

    if !search_rest.is_empty() {
        search_filters.push((
            SearchFilter::EitherRegex(Regex::new(&format!("(?i){search_rest}"))?),
            SearchFilterMode::Require,
        ));
    }

    Ok(search_filters)
}

/// Splits `search` by spaces, except for spaces within quotation marks (`"` or `'`)
/// that are preceded by a colon (`:`).
///
/// # Example:
///
/// ```
/// let search = split_search("A search scope:\"A scope\" for \"something cool\"");
/// assert_eq!(search.get(0), Some(Cow::Borrowed("A")));
/// assert_eq!(search.get(1), Some(Cow::Borrowed("search")));
/// assert_eq!(search.get(2), Some(Cow::Borrowed("scope:A scope")));
/// assert_eq!(search.get(3), Some(Cow::Borrowed("for")));
/// assert_eq!(search.get(4), Some(Cow::Borrowed("\"something")));
/// assert_eq!(search.get(5), Some(Cow::Borrowed("cool\"")));
/// assert_eq!(search.get(6), None);
/// ```
fn split_search(search: &'_ str) -> Vec<Cow<'_, str>> {
    let mut parts = Vec::new();

    let mut part_start = 0;
    let mut quote = None;
    let mut skip_next = false;
    for (char_i, (byte_i, c)) in search.char_indices().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if let Some((quote_char, quote_start)) = quote {
            if c == quote_char && matches!(search.chars().nth(char_i + 1), None | Some(' ')) {
                parts.push(Cow::Owned(format!(
                    "{}{}",
                    &search[part_start..quote_start],
                    &search[(quote_start + 1)..byte_i]
                )));
                part_start = byte_i + 2;
                quote = None;
                skip_next = true;
            }
            continue;
        }
        if (c == '"' || c == '\'') && search.chars().nth(char_i - 1) == Some(':') {
            quote = Some((c, byte_i));
            continue;
        }
        if c == ' ' {
            parts.push(Cow::Borrowed(&search[part_start..byte_i]));
            part_start = byte_i + 1;
            continue;
        }
    }
    if let Some(last_part) = search.get(part_start..) {
        parts.push(Cow::Borrowed(last_part));
    }

    parts
}

async fn metadata_api(
    State(store): State<Arc<Mutex<TranslationStore>>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    debug!("Request for '/metadata'");

    let store = store.lock().await;

    let mut scopes = HashMap::new();
    for provider in store.providers() {
        if let Some(group_name) = provider.group_name() {
            let scopes = scopes
                .entry(group_name)
                .or_insert(json!([]))
                .as_array_mut()
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

            scopes.push(json!({
                "id": provider.id(),
                "name": provider.name(),
                "downloaded": store.provider_caches.contains_key(provider.id()),
            }));
        } else {
            scopes.insert(provider.name(), json!(provider.id()));
        }
    }
    let scopes: Vec<serde_json::Value> = scopes
        .iter()
        .map(|(group_name, value)| {
            if let Some(id) = value.as_str() {
                json!({ "name": group_name, "id": id, "downloaded": store.provider_caches.contains_key(id), })
            } else {
                json!({ "name": group_name, "scopes": value })
            }
        })
        .collect();

    Ok(Json(json!({
        "scopes": scopes,
        "languages": store.languages(),
    })))
}
