// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use crate::Translation;

pub fn parse_json(text: String) -> anyhow::Result<Vec<Translation>> {
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
        });
    }
    Ok(translations)
}

pub fn parse_freeshow_json(
    text: String,
) -> anyhow::Result<HashMap<String, (String, Option<String>)>> {
    let json: HashMap<String, HashMap<String, String>> = serde_json::from_str(&text)?;

    let mut translations = HashMap::new();
    for (category_key, strings) in json {
        for (key, string) in strings {
            translations.insert(format!("{category_key}.{key}"), (string, None));
        }
    }
    Ok(translations)
}

pub fn parse_mastodon_json(
    text: String,
) -> anyhow::Result<HashMap<String, (String, Option<String>)>> {
    let json: HashMap<String, String> = serde_json::from_str(&text)?;
    let translations = json
        .into_iter()
        .map(|(key, value)| (key, (value, None)))
        .collect();
    Ok(translations)
}
