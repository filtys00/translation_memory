// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub fn parse_browser_extension(
    text: String,
) -> anyhow::Result<HashMap<String, (String, Option<String>)>> {
    let messages: HashMap<String, Message> = serde_json::from_str(&text)?;
    let messages = messages
        .into_iter()
        .map(|(key, message)| {
            let mut comment = key.clone();
            if let Some(placeholders) = message.placeholders {
                for (name, placeholder) in placeholders {
                    comment.push('\n');
                    comment.push_str(&name);
                    comment.push_str(": ");
                    comment.push_str(&placeholder.content);

                    if let Some(example) = &placeholder.example {
                        comment.push_str("\n    ");
                        comment.push_str(example);
                    }
                }
            }
            (key, (message.message, Some(comment)))
        })
        .collect();
    Ok(messages)
}

#[derive(Deserialize, Serialize)]
struct Message {
    message: String,
    placeholders: Option<HashMap<String, Placeholder>>,
}

#[derive(Deserialize, Serialize)]
struct Placeholder {
    content: String,
    example: Option<String>,
}

pub fn parse_dark_reader(
    text: String,
) -> anyhow::Result<HashMap<String, (String, Option<String>)>> {
    let mut translations: HashMap<String, (String, Option<String>)> = HashMap::new();

    let mut key: Option<String> = None;
    let mut value: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('@') {
            if let Some(key) = key {
                if let Some(value) = value {
                    translations.insert(
                        key.clone(),
                        (value.trim_end_matches('\n').to_string(), Some(key)),
                    );
                }
            }

            key = Some(line.to_string());
            value = None;
            continue;
        }

        let Some(value) = &mut value else {
            value = Some(line.to_string());
            continue;
        };

        value.push('\n');
        value.push_str(line);
    }

    if let Some(key) = key {
        if let Some(value) = value {
            translations.insert(
                key.clone(),
                (value.trim_end_matches('\n').to_string(), Some(key)),
            );
        }
    }

    Ok(translations)
}
