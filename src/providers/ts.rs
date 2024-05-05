// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};

use crate::Translation;

pub fn parse_qbittorrent_ts(text: String, source: &str) -> anyhow::Result<Vec<Translation>> {
    let translations: Ts = quick_xml::de::from_str(&text)?;

    let translations = translations
        .contexts
        .into_iter()
        .flat_map(|context| {
            context
                .messages
                .into_iter()
                .filter(|message| {
                    !matches!(
                        message.translation.translation_type,
                        Some(TranslationType::Unfinished)
                    )
                })
                .map(|message| Translation {
                    original: message.source.text,
                    translation: message.translation.text,
                    comment: message.comment.map(|comment| comment.text),
                    key: None,
                    source: source.to_string(),
                })
        })
        .collect();

    Ok(translations)
}

#[derive(Debug, Deserialize, Serialize)]
struct Ts {
    #[serde(rename = "context")]
    contexts: Vec<Context>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Context {
    name: Text,

    #[serde(rename = "message")]
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Message {
    #[serde(rename = "location", default)]
    locations: Vec<Location>,
    source: Text,
    comment: Option<Text>,
    translation: MessageTranslation,
}

#[derive(Debug, Deserialize, Serialize)]
struct Location {
    #[serde(rename = "@filename")]
    filename: String,
    #[serde(rename = "@line")]
    line: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct MessageTranslation {
    #[serde(rename = "@type")]
    translation_type: Option<TranslationType>,

    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum TranslationType {
    Unfinished,
    Vanished,
}

#[derive(Debug, Deserialize, Serialize)]
struct Text {
    #[serde(rename = "$text")]
    text: String,
}
