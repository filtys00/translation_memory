// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{borrow::Cow, sync::Arc};

use axum::{
    Json, Router,
    extract::{Form, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use log::{debug, trace};
use regex::Regex;
use rust_embed::Embed;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use tokio::{net::TcpListener, sync::Mutex};

use crate::database::{
    QueryCountOptions,
    QueryFilter as DbFilter,
    QueryFilterMode as DbMode,
    QueryOptions,
    TranslationStore,
};

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
static_page!(search_icon, "search.svg", "image/svg+xml");

#[derive(Debug, Deserialize, Serialize)]
struct QueryParams {
    search: Option<String>,

    #[serde(default, deserialize_with = "deserialize_opt_str_vec")]
    require_languages: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_opt_str_vec")]
    deny_languages: Option<Vec<String>>,

    #[serde(default, deserialize_with = "deserialize_opt_str_vec")]
    require_scopes: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_opt_str_vec")]
    deny_scopes: Option<Vec<String>>,

    limit: Option<u32>,
    skip: Option<u32>,

    count: Option<bool>,
}
fn deserialize_opt_str_vec<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error> where D: Deserializer<'de> {
    let Some(value) = Option::<String>::deserialize(deserializer)? else { return Ok(None); };
    if value.is_empty() { return Ok(None); }
    Ok(Some(value.split(',').map(|v| v.to_string()).collect()))
}

async fn query_api(
    State(store): State<Arc<Mutex<TranslationStore>>>,
    Form(params): Form<QueryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    debug!("Request for '/query': {params:?}");

    let mut filters = if let Some(search) = &params.search {
        parse_search(search).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    } else {
        vec![]
    };
    if let Some(codes) = params.require_scopes {
        filters.insert(0, (DbFilter::Providers { codes }, DbMode::Require));
    }
    if let Some(codes) = params.deny_scopes {
        filters.insert(0, (DbFilter::Providers { codes }, DbMode::Deny));
    }
    if let Some(lang_ids) = params.require_languages {
        filters.insert(0, (DbFilter::Languages { lang_ids }, DbMode::Require));
    }
    if let Some(lang_ids) = params.deny_languages {
        filters.insert(0, (DbFilter::Languages { lang_ids }, DbMode::Deny));
    }

    trace!("Search filters: {filters:?}");

    let store = store.lock().await;

    if params.count.unwrap_or(false) {
        let total_count = store.query_translation_count(QueryCountOptions { filters })
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        trace!("Request for '/query': returning {total_count}",);

        return Ok(Json(serde_json::to_value(total_count).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?));
    }

    let translations = store
        .query_translations(QueryOptions {
            limit: params.limit.unwrap_or(u32::MAX),
            offset: params.skip.unwrap_or(0),
            filters: filters.clone(),
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;


    let translations = translations.into_iter().map(|translation| {
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

        let original_regex = filters.iter()
            .filter_map(|filter| {
                if let (DbFilter::Original { regex }, DbMode::Require) = filter { return Some(regex); }
                if let (DbFilter::All      { regex }, DbMode::Require) = filter { return Some(regex); }
                None
            })
            .find(|regex| regex.is_match(&translation.original));
        let translation_regex = filters.iter()
            .filter_map(|filter| {
                if let (DbFilter::Translation { regex }, DbMode::Require) = filter { return Some(regex); }
                if let (DbFilter::All         { regex }, DbMode::Require) = filter { return Some(regex); }
                None
            })
            .find(|regex| regex.is_match(&translation.translation));

        let mut json = json!({
            "scope": translation.provider_code,
            "language": translation.language_id,
            "original": regex_parse(original_regex, &translation.original),
            "translation": regex_parse(translation_regex, &translation.translation),
            "source": translation.translations_url,
        });
        if let Some(comment) = &translation.comment {
            json["comment"] = json!(comment);
        }
        if let Some(key) = &translation.key {
            json["key"] = json!(key);
        }
        json
    });

    let translations: Vec<_> = translations.collect();

    trace!(
        "Request for '/query': returning {} translations",
        translations.len(),
    );

    Ok(Json(serde_json::to_value(translations).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

fn parse_search(search: &str) -> anyhow::Result<Vec<(DbFilter, DbMode)>> {
    if !search.contains(':') {
        if search.is_empty() {
            return Ok(vec![]);
        } else {
            return Ok(vec![(
                DbFilter::All { regex: Regex::new(&format!("(?i){search}"))? },
                DbMode::Require,
            )]);
        }
    }

    let mut search_filters = Vec::new();

    let mut search_rest = String::new();
    let mut first = true;
    for part in split_search(search) {
        if let Some((key, value)) = part.split_once(':') {
            let (key, mode) = if let Some(base_key) = key.strip_prefix('-') {
                (base_key, DbMode::Deny)
            } else {
                (key, DbMode::Require)
            };

            match key {
                "o" | "original" => {
                    search_filters.push((
                        DbFilter::Original { regex: Regex::new(&format!("(?i){value}"))? },
                        mode,
                    ));
                    continue;
                }
                "t" | "translation" => {
                    search_filters.push((
                        DbFilter::Translation { regex: Regex::new(&format!("(?i){value}"))? },
                        mode,
                    ));
                    continue;
                }
                "s" | "scope" => {
                    search_filters.push((DbFilter::Provider { name: value.to_string() }, mode));
                    continue;
                }
                "l" | "lang" | "language" => {
                    search_filters.push((DbFilter::Language { lang_id: value.to_string() }, mode));
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
            DbFilter::All { regex: Regex::new(&format!("(?i){search_rest}"))? },
            DbMode::Require,
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
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    debug!("Request for '/metadata'");

    let store = store.lock().await;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Provider { id: String, name: String, group_name: Option<String> }

    let mut scopes = Vec::new();

    let providers = store.get_providers()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    for provider in providers {
        let names = provider.get_names().
            map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        scopes.push(Provider { id: names.code, name: names.name, group_name: names.group_name });
    }

    let languages = store.get_languages()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({ "scopes": scopes, "languages": languages })))
}
