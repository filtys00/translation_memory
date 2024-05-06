// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::{anyhow, bail};

use super::{unescape, TranslationMessages};

pub fn parse_properties(text: String) -> anyhow::Result<TranslationMessages> {
    let mut translations = HashMap::new();

    let mut comment = None;
    for mut line in text.lines() {
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
        let value = unescape(value, &['n']);
        translations.insert(
            key.to_string(),
            (value, comment.map(|comment| comment.to_string())),
        );
    }

    Ok(translations)
}

pub fn parse_obs_studio_ini(text: String) -> anyhow::Result<TranslationMessages> {
    let mut translations = HashMap::new();

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            bail!("Invalid line: {line}");
        };

        let value = value
            .get(1..(value.len() - 1))
            .ok_or_else(|| anyhow!("Invalid value: {value}"))?;
        translations.insert(key.to_string(), (unescape(value, &['n', '"']), None));
    }

    Ok(translations)
}
