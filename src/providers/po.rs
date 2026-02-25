// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{collections::HashMap, iter};

use anyhow::anyhow;
use base64::{alphabet::Alphabet, engine::{GeneralPurpose, GeneralPurposeConfig}, Engine};
use log::trace;

use super::{Translation, unescape};

const BASE64: GeneralPurpose = GeneralPurpose::new(
    match &Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/") {
        Ok(alphabet) => alphabet,
        Err(_) => unreachable!(),
    },
    GeneralPurposeConfig::new(),
);

pub fn parse_po_base64(base64: String, source: &str) -> anyhow::Result<Vec<Translation>> {
    let bytes = BASE64
        .decode(&base64)
        .map_err(|e| anyhow!("Invalid base64: {e}\n{base64}"))?;
    let text =
        String::from_utf8(bytes).map_err(|e| anyhow!("Invalid text from base64: {e}\n{base64}"))?;
    parse_po(text, source)
}

pub fn parse_po(text: String, source: &str) -> anyhow::Result<Vec<Translation>> {
    let mut translations = Vec::new();

    let mut values: HashMap<&str, String> = HashMap::new();
    let mut last: Option<&mut String> = None;
    for line in text.lines().chain(iter::once("")) {
        if line.is_empty() {
            translations.push(Translation {
                original: {
                    let Some(msgid) = values.remove("msgid") else {
                        values.clear();
                        last = None;
                        continue;
                    };
                    if msgid.is_empty() {
                        values.clear();
                        last = None;
                        continue;
                    }
                    unescape(&msgid, &['n', 'u', 't', '"'])
                },
                translation: {
                    let Some(msgstr) = values.remove("msgstr") else {
                        values.clear();
                        last = None;
                        continue;
                    };
                    if msgstr.is_empty() {
                        values.clear();
                        last = None;
                        continue;
                    }
                    unescape(&msgstr, &['n', 'u', 't', '"'])
                },
                comment: values.remove("#.").or_else(|| values.remove("msgctxt")),
                key: None,
                source: source.to_string(),
            });

            values.clear();
            last = None;
            continue;
        } else if line.starts_with('"') && line.ends_with('"') && line.len() >= 2 {
            let line = &line[1..(line.len() - 1)];
            let Some(ref mut last) = last else {
                trace!("No last value: new value = {line}");
                continue;
            };
            last.push_str(line);
        } else if line.starts_with("# ") || (line.starts_with('#') && line.len() <= 2) {
            continue;
        } else if let Some((name, mut value)) = line.split_once(' ') {
            if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                value = &value[1..(value.len() - 1)];
            }
            let value = values.entry(name).or_insert(value.to_string());
            last = Some(value);
        } else {
            trace!("Unexpected line: {line}");
        }
    }

    Ok(translations)
}
