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
