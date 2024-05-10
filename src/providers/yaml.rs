// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::{anyhow, bail};

use super::TranslationMessages;

pub fn parse_mastodon_yaml(text: String) -> anyhow::Result<TranslationMessages> {
    let yaml: HashMap<String, HashMap<String, serde_yaml::Value>> = serde_yaml::from_str(&text)?;
    let values = yaml
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("No top-level key:\n{text}"))?
        .1;

    let mut messages = HashMap::new();
    let mut place = Vec::new();

    for (key, value) in values {
        place.push(key.to_string());
        parse_recursive(&value, &mut place, &mut messages)?;
        place.pop();
    }

    Ok(messages)
}

fn parse_recursive(
    value: &serde_yaml::Value,
    place: &mut Vec<String>,
    messages: &mut TranslationMessages,
) -> anyhow::Result<()> {
    if let Some(message) = value.as_str() {
        messages.insert(place.join("."), (message.to_string(), None));
        return Ok(());
    }

    let Some(mapping) = value.as_mapping() else {
        bail!("Unsupported type: {value:?}, supported types are 'str' and 'mapping'");
    };

    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            bail!("Unsupported key type: {key:?}, only the 'str' type is supported");
        };

        place.push(key.to_string());
        parse_recursive(value, place, messages)?;
        place.pop();
    }

    Ok(())
}
