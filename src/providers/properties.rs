// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::{anyhow, bail};

use super::{unescape, TranslationMessages};

pub fn parse_properties(text: String) -> anyhow::Result<TranslationMessages> {
    let mut messages = HashMap::new();

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

        let Some((mut key, mut message)) = line.split_once('=') else {
            bail!("Invalid line: {line}");
        };
        key = key.trim_end();
        message = message.trim_start();

        if messages.contains_key(key) {
            bail!("Duplicate key: {key}");
        }
        let message = unescape(message, &['n']);
        messages.insert(
            key.to_string(),
            (message, comment.map(|comment| comment.to_string())),
        );
    }

    Ok(messages)
}

pub fn parse_obs_studio_ini(text: String) -> anyhow::Result<TranslationMessages> {
    let mut messages = HashMap::new();

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let Some((key, message)) = line.split_once('=') else {
            bail!("Invalid line: {line}");
        };

        let message = message
            .get(1..(message.len() - 1))
            .ok_or_else(|| anyhow!("Invalid value: {message}"))?;
        let message = unescape(message, &['n', '"']);

        messages.insert(key.to_string(), (message, None));
    }

    Ok(messages)
}
