use std::{
    borrow::Cow,
    collections::HashMap,
    fs::File,
    io::{Read, Write as _},
    net::SocketAddr,
    path::Path,
    sync::Arc,
};

use axum::{
    extract::{Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router, Server,
};
use clap::Parser;
use env_logger::fmt::Color;
use log::{debug, error, info, Level, LevelFilter};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;
use translation_memory::{Translation, TranslationStore};
use unic_langid::LanguageIdentifier;

#[cfg(debug_assertions)]
const CACHE_PATH: &str = "translations.json";
#[cfg(not(debug_assertions))]
const CACHE_PATH: &str = "translations.bin";

#[derive(Debug, Parser)]
struct Args {
    /// Write all trace logs
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Write trace logs for a specific target. Example: `--log providers::android`
    #[arg(long = "log", value_delimiter = ',')]
    logs: Vec<String>,

    /// Don't open the system web browser when the web server starts
    #[arg(long, conflicts_with = "generate")]
    no_browser: bool,

    /// Generate one or more providers
    #[arg(long, value_delimiter = ',')]
    generate: Vec<String>,

    /// Languages to generate for
    #[arg(long, value_delimiter = ',', requires = "generate")]
    language: Vec<LanguageIdentifier>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let mut builder = env_logger::builder();
    builder.filter_module(
        env!("CARGO_PKG_NAME"),
        if args.verbose {
            LevelFilter::max()
        } else {
            LevelFilter::Debug
        },
    );
    for log in args.logs {
        builder.filter_module(
            &format!("{}::{log}", env!("CARGO_PKG_NAME")),
            LevelFilter::max(),
        );
    }
    builder
        .format(|buf, record| {
            let mut dimmed_style = buf.style();
            dimmed_style.set_color(Color::Black);
            dimmed_style.set_intense(true);

            writeln!(
                buf,
                "{}{} {}{} {}",
                dimmed_style.value('['),
                buf.default_styled_level(record.level()),
                record.target(),
                dimmed_style.value(']'),
                if let Level::Warn | Level::Info = record.level() {
                    record.args().to_string().replace('\n', "\n      ")
                } else {
                    record.args().to_string().replace('\n', "\n       ")
                }
            )
        })
        .init();

    let mut store = if Path::new(CACHE_PATH).exists() {
        info!("Reading caches translations from '{CACHE_PATH}'...");
        TranslationStore::from_file(CACHE_PATH).unwrap()
    } else {
        TranslationStore::default()
    };

    if !args.generate.is_empty() {
        let lang_ids = if args.language.is_empty() {
            store.languages().into_iter().cloned().collect()
        } else {
            args.language
        };
        if let Err(e) = store.generate(lang_ids, args.generate).await {
            error!("{e}");
        }
        if let Err(e) = store.write_to(CACHE_PATH) {
            error!("{e}");
        }
        return;
    }

    if !args.no_browser {
        info!("Opening web browser...");
        webbrowser::open("http://127.0.0.1:2013/").unwrap();
    }

    info!("Starting web server...");
    web_server(store).await
}

async fn web_server(store: TranslationStore) {
    let store = Arc::new(Mutex::new(store));

    let app = Router::new()
        .route("/", get(main_page))
        .route("/debug", get(debug_api))
        .route("/query", get(query_api))
        .route("/metadata", get(metadata_api))
        .route("/update", post(update_api))
        .route("/icon/search.svg", get(search_icon))
        .route("/icon/language.svg", get(language_icon))
        .route("/icon/loading.svg", get(loading_icon))
        .route("/icon/remove.svg", get(remove_icon))
        .route("/favicon.ico", get(language_icon))
        .with_state(store);

    let addr = SocketAddr::from(([127, 0, 0, 1], 2013));
    Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[cfg(debug_assertions)]
async fn main_page() -> Html<String> {
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

async fn debug_api(
    State(store): State<Arc<Mutex<TranslationStore>>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<String>)> {
    debug!(
        "Request for '/debug': {{ scope: {}, group: {}, language: {}, list: {}, count: {} }}",
        params
            .get("scope")
            .map(|scope| Cow::Owned(format!("\"{scope}\"")))
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .get("group")
            .map(|group| Cow::Owned(format!("\"{group}\"")))
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .get("language")
            .map(|language| Cow::Owned(format!("\"{language}\"")))
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .get("list")
            .map(|list| Cow::Borrowed(list.as_str()))
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .get("count")
            .map(|count| Cow::Borrowed(count.as_str()))
            .unwrap_or(Cow::Borrowed("undefined")),
    );

    let store = store.lock().await;

    let count = params
        .get("count")
        .map(|count| count.parse::<bool>())
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e.to_string())))?
        .unwrap_or(false);
    let list = params
        .get("list")
        .map(|list| list.parse::<bool>())
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e.to_string())))?
        .unwrap_or(false);
    let scope = params.get("scope");
    let group = params.get("group").map(|group| {
        if group == "null" {
            None
        } else {
            Some(group.as_str())
        }
    });
    let language = params
        .get("language")
        .map(|lang| lang.parse::<LanguageIdentifier>())
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(e.to_string())))?;

    if list | count {
        let translations: Vec<&Translation> = match (scope, group, language) {
            (None, None, None) => store
                .translations
                .iter()
                .flat_map(|(_, translations)| translations.iter())
                .filter_map(|(_, translations)| translations.as_ref())
                .flatten()
                .collect(),
            (Some(scope), _, None) => {
                let Some(translations) = store.translations.get(scope) else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(String::from("scope not found")),
                    ));
                };
                translations
                    .iter()
                    .filter_map(|(_, translation)| translation.as_ref())
                    .flatten()
                    .collect()
            }
            (None, None, Some(language)) => store
                .translations
                .iter()
                .filter_map(|(_, translations)| translations.get(&language)?.as_ref())
                .flatten()
                .collect(),
            (Some(scope), _, Some(language)) => {
                let Some(translations) = store.translations.get(scope) else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(String::from("scope not found")),
                    ));
                };
                let Some(Some(translations)) = translations.get(&language) else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(String::from("language not found")),
                    ));
                };
                translations.iter().collect()
            }
            (None, Some(group), None) => store
                .translations
                .iter()
                .filter(|(scope, _)| {
                    store
                        .provider(scope)
                        .map_or(false, |provider| provider.group_name() == group)
                })
                .flat_map(|(_, translations)| {
                    translations
                        .values()
                        .filter_map(|translations| translations.as_ref())
                })
                .flatten()
                .collect(),
            (None, Some(group), Some(language)) => store
                .translations
                .iter()
                .filter(|(scope, _)| {
                    store
                        .provider(scope)
                        .map_or(false, |provider| provider.group_name() == group)
                })
                .filter_map(|(_, translations)| translations.get(&language))
                .flatten()
                .flatten()
                .collect(),
        };

        let value = if count {
            serde_json::to_value(translations.len())
        } else {
            serde_json::to_value(translations)
        };

        value
            .map(Json)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))
    } else {
        let value = match (scope, group, language) {
            (None, None, None) => serde_json::to_value(&*store),
            (Some(scope), _, None) => {
                let Some(translations) = store.translations.get(scope) else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(String::from("scope not found")),
                    ));
                };

                serde_json::to_value(translations)
            }
            (Some(scope), _, Some(language)) => {
                let Some(translations) = store.translations.get(scope) else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(String::from("scope not found")),
                    ));
                };
                let Some(Some(translations)) = translations.get(&language) else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(String::from("language not found")),
                    ));
                };

                serde_json::to_value(translations)
            }
            (None, None, Some(language)) => {
                let translations: HashMap<_, _> = store
                    .translations
                    .iter()
                    .filter_map(|(scope, translations)| {
                        translations
                            .get(&language)
                            .map(|language| (scope, language))
                    })
                    .collect();

                serde_json::to_value(translations)
            }
            (None, Some(group), None) => {
                let translations: HashMap<_, _> = store
                    .translations
                    .iter()
                    .filter(|(scope, _)| {
                        store
                            .provider(scope)
                            .map_or(false, |provider| provider.group_name() == group)
                    })
                    .collect();

                serde_json::to_value(translations)
            }
            (None, Some(group), Some(language)) => {
                let translations: HashMap<_, _> = store
                    .translations
                    .iter()
                    .filter_map(|(scope, translations)| {
                        if store.provider(scope)?.group_name() == group {
                            return None;
                        }
                        translations
                            .get(&language)?
                            .as_ref()
                            .map(|translation| (scope, translation))
                    })
                    .collect();

                serde_json::to_value(translations)
            }
        };

        value
            .map(Json)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())))
    }
}

async fn query_api(
    State(store): State<Arc<Mutex<TranslationStore>>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let scopes: Option<Vec<&str>> = params
        .get("scopes")
        .map(|scopes| scopes.split(',').collect());

    debug!(
        "Request for '/query': {{ regex: {}, languages: {}, scopes: {}, limit: {} }}",
        params
            .get("regex")
            .map(|v| Cow::Owned(format!("\"{v}\"")))
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .get("languages")
            .map(|v| Cow::Owned(format!("\"{v}\"")))
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .get("scopes")
            .map(|v| {
                let scopes = scopes.as_ref().map(|s| s.len()).unwrap_or(0);

                Cow::Owned(if scopes > 3 {
                    format!("<{scopes}>")
                } else {
                    format!("\"{v}\"")
                })
            })
            .unwrap_or(Cow::Borrowed("undefined")),
        params
            .get("limit")
            .map(|v| Cow::Borrowed(v.as_str()))
            .unwrap_or(Cow::Borrowed("undefined")),
    );

    let regex = if let Some(regex) = params.get("regex") {
        Some(
            Regex::new(&format!("(?i){regex}"))
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
        )
    } else {
        None
    };

    let langs: Option<Vec<LanguageIdentifier>> = if let Some(langs) = params.get("languages") {
        let mut langs_parsed = Vec::new();
        for lang in langs.split(',') {
            langs_parsed.push(
                lang.parse::<LanguageIdentifier>()
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
            );
        }
        Some(langs_parsed)
    } else {
        None
    };

    let limit = if let Some(limit) = params.get("limit") {
        Some(
            limit
                .parse::<usize>()
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
        )
    } else {
        None
    };

    let store = store.lock().await;
    let translations = store
        .iter()
        .filter(|(scope, lang_id, translation)| {
            if let Some(regex) = &regex {
                if !regex.is_match(&translation.original)
                    && !regex.is_match(&translation.translation)
                {
                    return false;
                }
            }
            if let Some(scopes) = &scopes {
                if !scopes.contains(&scope.as_str()) {
                    return false;
                }
            }
            if let Some(langs) = &langs {
                if !langs.contains(lang_id) {
                    return false;
                }
            }

            true
        })
        .map(|(scope, lang_id, translation)| {
            if let Some(comment) = &translation.comment {
                json!({
                    "scope": scope,
                    "language": lang_id,
                    "comment": comment,
                    "original": translation.original,
                    "translation": translation.translation,
                })
            } else {
                json!({
                    "scope": scope,
                    "language": lang_id,
                    "original": translation.original,
                    "translation": translation.translation,
                })
            }
        });

    let translations: Vec<serde_json::Value> = if let Some(limit) = limit {
        translations.take(limit).collect()
    } else {
        translations.collect()
    };

    Ok(Json(serde_json::to_value(translations).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

async fn metadata_api(
    State(store): State<Arc<Mutex<TranslationStore>>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    debug!("Request for '/metadata'");

    let store = store.lock().await;

    let mut scopes = HashMap::with_capacity(store.providers().len());
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
                "downloaded": store.translations.contains_key(provider.id()),
            }));
        } else {
            scopes.insert(provider.name(), json!(provider.id()));
        }
    }
    let scopes: Vec<serde_json::Value> = scopes
        .iter()
        .map(|(group_name, value)| {
            if let Some(id) = value.as_str() {
                json!({ "name": group_name, "id": id, "downloaded": store.translations.contains_key(id), })
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

    let errors = match store.generate(payload.languages, payload.scopes).await {
        Err(e) => {
            error!("Could not generate: {e}");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
        Ok(errors) => errors,
    };

    debug!("Writing translations to disk");
    if let Err(e) = store.write_to(CACHE_PATH) {
        error!("Could not save transaltions: {e}");
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
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
