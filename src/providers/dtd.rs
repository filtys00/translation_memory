// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::bail;

use super::TranslationMessages;

pub fn parse_dtd(text: String) -> anyhow::Result<TranslationMessages> {
    let mut messages = HashMap::new();

    let mut comment = (false, None);
    for mut line in text.lines() {
        line = line.trim();

        if comment.0 {
            if line.ends_with("-->") {
                comment = (false, None);
            }
            continue;
        }

        if line.is_empty() {
            comment = (false, None);
            continue;
        }

        if let Some(line) = line.strip_prefix("<!--") {
            if let Some(line) = line.strip_suffix("-->") {
                comment = (false, Some(line.trim()));
            } else {
                comment = (true, None);
            }
            continue;
        }

        let entity = line.strip_prefix("<!ENTITY ")
            .and_then(|line| line.strip_suffix('>'))
            .and_then(|line| line.trim().split_once(' '));
        if let Some((key, value)) = entity {
            if messages.contains_key(key) { bail!("Duplicate key: {key}"); }

            let message = value.trim_start().strip_prefix('"')
                .and_then(|v| v.trim_end().strip_suffix('"'));
            if let Some(message) = message {
                messages.insert(
                    key.to_string(),
                    (message.to_string(), comment.1.map(|c| c.to_string())),
                );
                comment = (false, None);
                continue;
            }
        }

        bail!("Unexpected line: {line}");
    }

    Ok(messages)
}
