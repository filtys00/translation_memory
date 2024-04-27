// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::{anyhow, bail};

use super::TranslationMessages;

pub fn parse_mastodon_yaml(text: String) -> anyhow::Result<TranslationMessages> {
    let yaml: HashMap<String, HashMap<String, serde_yaml::Value>> = serde_yaml::from_str(&text)?;
    let yaml = yaml
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("No top-level key:\n{text}"))?
        .1;

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
    value: &serde_yaml::Value,
    place: &mut Vec<String>,
    translations: &mut TranslationMessages,
) -> anyhow::Result<()> {
    if let Some(value) = value.as_str() {
        translations.insert(place.join("."), (value.to_string(), None));
        return Ok(());
    }

    let Some(value) = value.as_mapping() else {
        bail!("Unsupported type: {value:?}, supported types are 'str' and 'mapping'");
    };

    for (key, value) in value {
        let Some(key) = key.as_str() else {
            bail!("Unsupported key type: {key:?}, only the 'str' type is supported");
        };

        place.push(key.to_string());
        parse_recursive(value, place, translations)?;
        place.pop();
    }

    Ok(())
}
