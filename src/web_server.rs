// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
};

use axum::{
    extract::{Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use log::{debug, error, trace};
use regex::Regex;
use serde::{
    de::{self, Unexpected},
    Deserialize, Deserializer, Serialize,
};
use serde_json::json;
use tokio::{net::TcpListener, sync::Mutex};
use translation_memory::TranslationStore;
use unic_langid::LanguageIdentifier;

pub async fn web_server(store: Arc<Mutex<TranslationStore>>) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(main_page))
        .route("/query", get(query_api))
        .route("/metadata", get(metadata_api))
        .route("/update", post(update_api))
        .route("/update_all", post(update_all_api))
        .route("/icon/search.svg", get(search_icon))
        .route("/icon/language.svg", get(language_icon))
        .route("/icon/loading.svg", get(loading_icon))
        .route("/icon/remove.svg", get(remove_icon))
        .route("/favicon.ico", get(language_icon))
        .with_state(store);

    let listener = TcpListener::bind("127.0.0.1:2013").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(debug_assertions)]
async fn main_page() -> Html<String> {
    use std::{fs::File, io::Read};

    debug!("Request for '/' (read from file)");

    let mut file = File::open("src/page.html").unwrap();
    let mut page = String::new();
    file.read_to_string(&mut page).unwrap();
    Html(page)
}

#[cfg(not(debug_assertions))]
async fn main_page() -> Html<&'static str> {
    debug!("Request for '/'");
    Html(include_str!("page.html"))
}

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
fn split_search(search: &str) -> Vec<Cow<str>> {
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

#[derive(Debug, Deserialize, Serialize)]
struct UpdatePayload {
    languages: Vec<LanguageIdentifier>,
    scopes: Vec<String>,
}

async fn update_api(
    State(store): State<Arc<Mutex<TranslationStore>>>,
    Json(payload): Json<UpdatePayload>,
) -> Result<Json<HashMap<String, Option<String>>>, (StatusCode, String)> {
    debug!(
        "Request for '/update':\
        \n{{\
        \n    languages: [{}],\
        \n    scopes: [{}\
        \n    ]\
        \n}}",
        payload
            .languages
            .iter()
            .map(|lang| format!("\"{lang}\""))
            .reduce(|a, b| a + ", " + &b)
            .unwrap_or_default(),
        payload.scopes.iter().fold(String::new(), |acc, scope| acc
            + "\n        \""
            + scope
            + "\","),
    );

    let mut store = store.lock().await;

    let errors = match store
        .generate(payload.languages, payload.scopes, false)
        .await
    {
        Err(e) => {
            error!("Could not generate: {e}");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
        Ok(errors) => errors,
    };

    if errors.values().any(|error| error.is_none()) {
        debug!("Writing translations to disk");
        if let Err(e) = store.save_translations() {
            error!("Could not save translations: {e}");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
    } else {
        debug!("Skipping writing translations to disk; no translations were updated");
    }

    Ok(Json(errors))
}

#[derive(Debug, Deserialize, Serialize)]
struct UpdateAllPayload {
    languages: Vec<LanguageIdentifier>,
}

async fn update_all_api(
    State(store): State<Arc<Mutex<TranslationStore>>>,
    Json(payload): Json<UpdateAllPayload>,
) -> Result<Json<HashMap<String, Option<String>>>, (StatusCode, String)> {
    debug!(
        "Request for '/update_all':\
        \n{{\
        \n    languages: [{}]\
        \n}}",
        payload
            .languages
            .iter()
            .map(|lang| format!("\"{lang}\""))
            .reduce(|a, b| a + ", " + &b)
            .unwrap_or_default(),
    );

    let mut store = store.lock().await;

    let scopes: Vec<String> = store
        .providers()
        .map(|provider| provider.id().to_string())
        .collect();

    let errors = if payload
        .languages
        .iter()
        .collect::<HashSet<_>>()
        .difference(&store.languages())
        .count()
        == 0
    {
        debug!("Fullfilling request by removal");
        for provider_cache in store.provider_caches.values_mut() {
            for translation_bundle in provider_cache.translation_bundles_mut() {
                translation_bundle.retain(|lang_id, _| payload.languages.contains(lang_id));
            }
        }
        scopes.into_iter().map(|scope| (scope, None)).collect()
    } else {
        debug!("Fullfilling request by generation");
        match store.generate(payload.languages, scopes, true).await {
            Ok(errors) => errors,
            Err(e) => {
                error!("Could not generate: {e}");
                return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
            }
        }
    };

    if errors.values().any(|error| error.is_none()) {
        debug!("Writing translations to disk");
        if let Err(e) = store.save_translations() {
            error!("Could not save translations: {e}");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
    } else {
        debug!("Skipping writing translations to disk; no translations were updated");
    }

    Ok(Json(errors))
}

async fn search_icon() -> Response {
    debug!("Request for '/icon/search.svg'");
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/svg+xml"),
        )],
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--! Font Awesome Free 6.4.2 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free (Icons: CC BY 4.0, Fonts: SIL OFL 1.1, Code: MIT License) Copyright 2023 Fonticons, Inc. --><path d="M416 208c0 45.9-14.9 88.3-40 122.7L502.6 457.4c12.5 12.5 12.5 32.8 0 45.3s-32.8 12.5-45.3 0L330.7 376c-34.4 25.2-76.8 40-122.7 40C93.1 416 0 322.9 0 208S93.1 0 208 0S416 93.1 416 208zM208 352a144 144 0 1 0 0-288 144 144 0 1 0 0 288z"/></svg>"#,
    ).into_response()
}

async fn language_icon() -> Response {
    debug!("Request for '/icon/language.svg'");
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/svg+xml"),
        )],
        r#"<svg xmlns="http://www.w3.org/2000/svg" height="1em" viewBox="0 0 640 512"><!--! Font Awesome Free 6.4.2 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license (Commercial License) Copyright 2023 Fonticons, Inc. --><path d="M0 128C0 92.7 28.7 64 64 64H256h48 16H576c35.3 0 64 28.7 64 64V384c0 35.3-28.7 64-64 64H320 304 256 64c-35.3 0-64-28.7-64-64V128zm320 0V384H576V128H320zM178.3 175.9c-3.2-7.2-10.4-11.9-18.3-11.9s-15.1 4.7-18.3 11.9l-64 144c-4.5 10.1 .1 21.9 10.2 26.4s21.9-.1 26.4-10.2l8.9-20.1h73.6l8.9 20.1c4.5 10.1 16.3 14.6 26.4 10.2s14.6-16.3 10.2-26.4l-64-144zM160 233.2L179 276H141l19-42.8zM448 164c11 0 20 9 20 20v4h44 16c11 0 20 9 20 20s-9 20-20 20h-2l-1.6 4.5c-8.9 24.4-22.4 46.6-39.6 65.4c.9 .6 1.8 1.1 2.7 1.6l18.9 11.3c9.5 5.7 12.5 18 6.9 27.4s-18 12.5-27.4 6.9l-18.9-11.3c-4.5-2.7-8.8-5.5-13.1-8.5c-10.6 7.5-21.9 14-34 19.4l-3.6 1.6c-10.1 4.5-21.9-.1-26.4-10.2s.1-21.9 10.2-26.4l3.6-1.6c6.4-2.9 12.6-6.1 18.5-9.8l-12.2-12.2c-7.8-7.8-7.8-20.5 0-28.3s20.5-7.8 28.3 0l14.6 14.6 .5 .5c12.4-13.1 22.5-28.3 29.8-45H448 376c-11 0-20-9-20-20s9-20 20-20h52v-4c0-11 9-20 20-20z"/></svg>"#,
    ).into_response()
}

async fn loading_icon() -> Response {
    debug!("Request for '/icon/loading.svg'");
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/svg+xml"),
        )],
        r#"<svg xmlns="http://www.w3.org/2000/svg" height="16" width="16" viewBox="0 0 512 512"><!--!Font Awesome Free 6.5.1 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2023 Fonticons, Inc.--><path d="M304 48a48 48 0 1 0 -96 0 48 48 0 1 0 96 0zm0 416a48 48 0 1 0 -96 0 48 48 0 1 0 96 0zM48 304a48 48 0 1 0 0-96 48 48 0 1 0 0 96zm464-48a48 48 0 1 0 -96 0 48 48 0 1 0 96 0zM142.9 437A48 48 0 1 0 75 369.1 48 48 0 1 0 142.9 437zm0-294.2A48 48 0 1 0 75 75a48 48 0 1 0 67.9 67.9zM369.1 437A48 48 0 1 0 437 369.1 48 48 0 1 0 369.1 437z"/></svg>"#,
    ).into_response()
}

async fn remove_icon() -> Response {
    debug!("Request for '/icon/remove.svg'");
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/svg+xml"),
        )],
        r#"<svg xmlns="http://www.w3.org/2000/svg" height="1em" viewBox="0 0 384 512" fill="white"><!--! Font Awesome Free 6.4.2 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license (Commercial License) Copyright 2023 Fonticons, Inc. --><path d="M342.6 150.6c12.5-12.5 12.5-32.8 0-45.3s-32.8-12.5-45.3 0L192 210.7 86.6 105.4c-12.5-12.5-32.8-12.5-45.3 0s-12.5 32.8 0 45.3L146.7 256 41.4 361.4c-12.5 12.5-12.5 32.8 0 45.3s32.8 12.5 45.3 0L192 301.3 297.4 406.6c12.5 12.5 32.8 12.5 45.3 0s12.5-32.8 0-45.3L237.3 256 342.6 150.6z"/></svg>"#,
    ).into_response()
}
