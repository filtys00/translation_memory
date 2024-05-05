// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::bail;
use regex::Regex;

use super::{unescape, TranslationMessages};
use crate::Translation;

pub fn parse_json(text: String) -> anyhow::Result<TranslationMessages> {
    let yaml: HashMap<String, serde_json::Value> = serde_json::from_str(&text)?;
    let mut translations = HashMap::new();
    let mut place = Vec::new();

    for (key, value) in yaml {
        place.push(key.to_string());
        parse_recursive(&value, &mut place, &mut translations)?;
        place.pop();
    }

    Ok(translations)
}

fn parse_recursive(
    value: &serde_json::Value,
    place: &mut Vec<String>,
    translations: &mut TranslationMessages,
) -> anyhow::Result<()> {
    if let Some(value) = value.as_str() {
        translations.insert(place.join("."), (value.to_string(), None));
        return Ok(());
    }

    let Some(value) = value.as_object() else {
        bail!("Unsupported type: {value:?}, supported types are 'string' and 'object'");
    };

    for (key, value) in value {
        place.push(key.to_string());
        parse_recursive(value, place, translations)?;
        place.pop();
    }

    Ok(())
}

pub fn parse_elementary_json(text: String, source: &str) -> anyhow::Result<Vec<Translation>> {
    let json: HashMap<String, String> = serde_json::from_str(&text)?;

    let mut translations = Vec::new();
    for (original, translation) in json {
        if original.is_empty() || translation.is_empty() {
            continue;
        }
        translations.push(Translation {
            original,
            translation,
            comment: None,
            key: None,
            source: source.to_string(),
        });
    }
    Ok(translations)
}

pub fn parse_geogebra_js_json(text: String) -> anyhow::Result<TranslationMessages> {
    let mut messages = TranslationMessages::new();

    let regex = Regex::new("JSON\\.parse\\(\"(.*)\"\\)").unwrap();
    for capture in regex.captures_iter(&text) {
        let Some(json) = capture.get(1) else {
            continue;
        };
        let json = unescape(json.as_str(), &['"', '\\']);
        let json: HashMap<String, String> = serde_json::from_str(&json)?;
        messages.extend(json.into_iter().map(|(key, value)| (key, (value, None))));
    }

    Ok(messages)
}
