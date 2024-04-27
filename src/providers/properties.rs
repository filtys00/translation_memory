// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::bail;

use super::TranslationMessages;

pub fn parse_properties(text: String) -> anyhow::Result<TranslationMessages> {
    let mut translations = HashMap::new();

    let mut comment = None;
    for mut line in text.split('\n') {
        if line.is_empty() {
            continue;
        }

        line = line.trim_start();
        if let Some(line) = line.strip_prefix('#') {
            comment = Some(line.trim_start());
            continue;
        }

        let Some((mut key, mut value)) = line.split_once('=') else {
            bail!("Invalid line: {line}");
        };
        key = key.trim_end();
        value = value.trim_start();

        if translations.contains_key(key) {
            bail!("Duplicate key: {key}");
        }
        translations.insert(
            key.to_string(),
            (
                value.to_string(),
                comment.map(|comment| comment.to_string()),
            ),
        );
    }

    Ok(translations)
}
