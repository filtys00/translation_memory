// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use anyhow::{anyhow, bail};
use base64::{
    alphabet::Alphabet,
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use quick_xml::{events::Event, Reader};

const BASE64: GeneralPurpose = GeneralPurpose::new(
    match &Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/") {
        Ok(alphabet) => alphabet,
        Err(_) => unreachable!(),
    },
    GeneralPurposeConfig::new(),
);

pub fn parse_android_base64(
    base64: String,
) -> anyhow::Result<HashMap<String, (String, Option<String>)>> {
    let bytes = BASE64
        .decode(&base64)
        .map_err(|e| anyhow!("Invalid base64: {e}\n{base64}"))?;
    let text =
        String::from_utf8(bytes).map_err(|e| anyhow!("Invalid text from base64: {e}\n{base64}"))?;
    parse_android(text)
}

pub fn parse_android(text: String) -> anyhow::Result<HashMap<String, (String, Option<String>)>> {
    let mut resources = HashMap::new();

    let mut reader = Reader::from_str(&text);
    let mut comment: Option<String> = None;
    let mut name: Option<String> = None;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Err(e) => bail!("{e}"),
            Ok(Event::Eof) => break,
            Ok(Event::Comment(e)) => {
                comment = Some(String::from_utf8_lossy(&e).trim().to_string());
            }
            Ok(Event::Start(e)) if e.name().as_ref() == b"string" => {
                if e.attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"translatable")
                    .map_or(false, |attr| attr.value.as_ref() == b"false")
                {
                    comment = None;
                    continue;
                };
                let Some(name_attr) = e
                    .attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"name")
                    .and_then(|attr| String::from_utf8(attr.value.to_vec()).ok())
                else {
                    comment = None;
                    continue;
                };
                name = Some(name_attr);
            }
            Ok(Event::Text(e)) if name.is_some() => {
                let t = String::from_utf8_lossy(&e);
                if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
                    text.push_str(&t[1..t.len() - 1]);
                } else {
                    text.push_str(t.trim());
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"string" => {
                let Some(n) = name else {
                    comment = None;
                    text.clear();
                    continue;
                };

                resources.insert(n, (text, comment));

                comment = None;
                name = None;
                text = String::new();
            }
            _ if name.is_some() => comment = None,
            _ => {}
        }
    }

    Ok(resources)
}
