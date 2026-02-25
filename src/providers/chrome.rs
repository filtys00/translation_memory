// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{collections::HashMap, str::FromStr};

use anyhow::{anyhow, bail};
use base64::{
    alphabet::Alphabet,
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use log::trace;
use quick_xml::{events::Event, Reader};
use regex::Regex;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use unic_langid::LanguageIdentifier;

use super::{download_text, unescape, TranslationMessages};

const BASE64: GeneralPurpose = GeneralPurpose::new(
    match &Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/") {
        Ok(alphabet) => alphabet,
        Err(_) => unreachable!(),
    },
    GeneralPurposeConfig::new(),
);

pub fn parse_xtb_base64(base64: String) -> anyhow::Result<TranslationMessages> {
    let bytes = BASE64
        .decode(&base64)
        .map_err(|e| anyhow!("Invalid base64: {e}\n{base64}"))?;
    let text = String::from_utf8(bytes)
        .map_err(|e| anyhow!("Invalid UTF-8 from base64: {e}\n{base64}"))?;
    parse_xtb(text)
}

pub fn parse_xtb(text: String) -> anyhow::Result<TranslationMessages> {
    let mut messages = HashMap::new();

    let mut reader = Reader::from_str(&text);
    let mut key: Option<String> = None;
    let mut message = String::new();
    loop {
        match reader.read_event()? {
            Event::Eof => break,

            Event::Start(e) if e.name().as_ref() == b"translationbundle" => {}
            Event::End(e) if e.name().as_ref() == b"translationbundle" => {}

            Event::Start(e) if e.name().as_ref() == b"translation" => {
                let Some(attribute) = e
                    .attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"id")
                else {
                    continue;
                };

                key = Some(String::from_utf8(attribute.value.to_vec())?);
                message.clear();
            }
            Event::End(e) if e.name().as_ref() == b"translation" => {
                if let Some(key) = key {
                    let message = unescape(&message, &['n', 'u']);
                    messages.insert(key, (message, None));
                }

                key = None;
                message = String::new();
            }
            Event::Text(bytes) => message.push_str(&String::from_utf8(bytes.to_vec())?),
            Event::Empty(e) if e.name().as_ref() == b"ph" => {
                let Some(attribute) = e
                    .attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"name")
                else {
                    continue;
                };

                message.push_str(&format!(
                    "<ph name=\"{}\" />",
                    String::from_utf8(attribute.value.to_vec())?
                ))
            }

            Event::Start(e) => {
                bail!(
                    "unexpected tag: <{}>",
                    String::from_utf8(e.name().as_ref().to_vec())?
                );
            }
            Event::End(e) => {
                bail!(
                    "unexpected tag: </{}>",
                    String::from_utf8(e.name().as_ref().to_vec())?
                );
            }
            Event::Empty(e) => {
                bail!(
                    "unexpected tag: <{} />",
                    String::from_utf8(e.name().as_ref().to_vec())?
                );
            }
            _ => {}
        }
    }

    Ok(messages)
}

pub async fn chromium_urls(
    lang_ids: Vec<LanguageIdentifier>,
    client: Client,
) -> anyhow::Result<HashMap<String, (Url, HashMap<LanguageIdentifier, Option<Url>>)>> {
    trace!("Downloading translation expectations...");

    let url = "https://chromium.googlesource.com/chromium/src/+/main/tools/gritsettings/translation_expectations.pyl?format=TEXT";
    let pyl_base64 = download_text(url, &client)
        .await
        .map_err(|e| anyhow!("Could not download translation expectations: {e}\n{url}"))?
        .ok_or_else(|| anyhow!("Could not find translation expectations\n{url}"))?;
    let pyl_bytes = BASE64.decode(&pyl_base64).map_err(|e| {
        anyhow!(
            "Could not parse translation expectations: invalid base64: {e}\n{url}\n{pyl_base64}"
        )
    })?;
    let pyl = String::from_utf8(pyl_bytes)
        .map_err(|e| anyhow!("Could not parse translation expectations: invalid UTF-8 from base64: {e}\n{url}\n{pyl_base64}"))?;
    let pyl = Regex::new("#.*\n")?.replace_all(&pyl, "");
    let pyl = Regex::new(r",\s*}")?.replace_all(&pyl, "}");
    let pyl = Regex::new(r",\s*]")?.replace_all(&pyl, "]");
    let translation_expectations: TranslationExpectations = serde_json::from_str(&pyl)
        .map_err(|e| anyhow!("Could not parse translation expectations: {e}\n{url}"))?;

    let mut join_set = JoinSet::new();

    let default_lang_id = LanguageIdentifier::from_str("en-GB")?;
    for grds in translation_expectations.other_grds.into_values() {
        if !grds.languages.contains(&default_lang_id)
            || !lang_ids
                .iter()
                .any(|lang_id| grds.languages.contains(lang_id))
        {
            continue;
        }

        for file in grds.files {
            let client = client.clone();
            let default_lang_id = default_lang_id.clone();
            let lang_ids = lang_ids.clone();

            join_set.spawn(async move {
                let grd_url = format!(
                    "https://chromium.googlesource.com/chromium/src/+/main/{file}?format=TEXT"
                );
                let grd_base64 = download_text(&grd_url, &client)
                    .await
                    .map_err(|e| anyhow!("Could not download 'grd' file: {e}\n{url}"))?
                    .ok_or_else(|| anyhow!("Could not find 'grd' file\n{url}"))?;
                let grd_bytes = BASE64
                    .decode(&grd_base64)
                    .map_err(|e| anyhow!("Could not parse 'grd' file: invalid base64: {e}\n{grd_url}\n{grd_base64}"))?;
                let grd =
                    String::from_utf8(grd_bytes).map_err(|e| anyhow!("Could not parse 'grd' file: invalid UTF-8 from base64: {e}\n{grd_url}\n{grd_base64}"))?;
                let grd: Grd = quick_xml::de::from_str(&grd)
                    .map_err(|e| anyhow!("Could not parse 'grd' file: {e}\n{url}"))?;

                let Some(default_translations_file) = grd
                    .translations
                    .entries
                    .iter()
                    .find(|grit| grit.lang == default_lang_id)
                else {
                    bail!("Could not find the default translation\n{url}");
                };
                let default_url = Url::parse(&format!(
                    "https://chromium.googlesource.com/chromium/src/+/main/{}/{}?format=TEXT",
                    file.rsplit_once('/').map(|(file, _)| file).unwrap_or(""),
                    default_translations_file.path
                ))?;

                let mut url_bundle = HashMap::new();

                for lang_id in &lang_ids {
                    let Some(translation_file) = grd
                        .translations
                        .entries
                        .iter()
                        .find(|grit| grit.lang == *lang_id)
                    else {
                        url_bundle.insert(lang_id.clone(), None);
                        continue;
                    };

                    let url = format!(
                        "https://chromium.googlesource.com/chromium/src/+/main/{}/{}?format=TEXT",
                        file.rsplit_once('/').map(|(file, _)| file).unwrap_or(""),
                        translation_file.path,
                    );

                    url_bundle.insert(lang_id.clone(), Some(Url::parse(&url)?));
                }

                Ok((default_url, url_bundle))
            });
        }
    }

    let mut url_bundles = HashMap::new();

    while let Some(result) = join_set.join_next().await {
        let result: anyhow::Result<(Url, HashMap<LanguageIdentifier, Option<Url>>)> = result?;
        let (default_url, url_bundle) = result?;

        url_bundles.insert(default_url.as_str().to_string(), (default_url, url_bundle));
    }

    Ok(url_bundles)
}

#[derive(Debug, Deserialize, Serialize)]
struct TranslationExpectations {
    untranslated_grds: HashMap<String, String>,
    internal_grds: Vec<String>,

    #[serde(flatten)]
    other_grds: HashMap<String, TranslationExpectationsGrds>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TranslationExpectationsGrds {
    languages: Vec<LanguageIdentifier>,
    files: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Grd {
    translations: GrdTranslations,
}

#[derive(Debug, Deserialize, Serialize)]
struct GrdTranslations {
    #[serde(rename = "file")]
    entries: Vec<GrdTranslationFile>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GrdTranslationFile {
    #[serde(rename = "@path")]
    path: String,
    #[serde(rename = "@lang")]
    lang: LanguageIdentifier,
}
