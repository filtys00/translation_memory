// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::bail;
use regex::Regex;

use super::{DbTranslation, TranslationMessages, unescape};

pub fn parse_json(text: String) -> anyhow::Result<TranslationMessages> {
    let json: HashMap<String, serde_json::Value> = serde_json::from_str(&text)?;

    let mut messages = HashMap::new();
    let mut place = Vec::new();

    for (key, value) in json {
        place.push(key.to_string());
        parse_recursive(&value, &mut place, &mut messages)?;
        place.pop();
    }

    Ok(messages)
}

fn parse_recursive(
    value: &serde_json::Value,
    place: &mut Vec<String>,
    messages: &mut TranslationMessages,
) -> anyhow::Result<()> {
    if let Some(message) = value.as_str() {
        messages.insert(place.join("."), (message.to_string(), None));
        return Ok(());
    }

    let Some(object) = value.as_object() else {
        bail!("Unsupported type: {value:?}, supported types are 'string' and 'object'");
    };

    for (key, value) in object {
        place.push(key.to_string());
        parse_recursive(value, place, messages)?;
        place.pop();
    }

    Ok(())
}

pub fn parse_elementary_json(text: String) -> anyhow::Result<Vec<DbTranslation>> {
    let json: HashMap<String, String> = serde_json::from_str(&text)?;

    let mut translations = Vec::new();
    for (message_en, message) in json {
        if message_en.is_empty() || message.is_empty() {
            continue;
        }
        translations.push(DbTranslation {
            original: message_en,
            translation: message,
            comment: None,
            key: None,
        });
    }
    Ok(translations)
}

pub fn parse_geogebra_js_json(text: String) -> anyhow::Result<TranslationMessages> {
    let mut messages = HashMap::new();

    let regex = Regex::new("JSON\\.parse\\(\"(.*)\"\\)").unwrap();
    for capture in regex.captures_iter(&text) {
        let Some(json) = capture.get(1) else {
            continue;
        };
        let json = unescape(json.as_str(), &['"', '\\']);
        let json: HashMap<String, String> = serde_json::from_str(&json)?;

        messages.extend(
            json.into_iter()
                .map(|(key, message)| (key, (message, None))),
        );
    }

    Ok(messages)
}
