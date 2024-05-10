// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use base64::{
    alphabet::Alphabet,
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use log::trace;
use quick_xml::{events::Event, Reader};
use regex::Regex;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use super::unescape;
use crate::{
    ProviderCache, ProviderCacheMultiple, Translation, TranslationBundle, TranslationProvider,
};

const BASE64: GeneralPurpose = GeneralPurpose::new(
    match &Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/") {
        Ok(alphabet) => alphabet,
        Err(_) => unreachable!(),
    },
    GeneralPurposeConfig::new(),
);

pub struct ChromeProvider;

#[async_trait]
impl TranslationProvider for ChromeProvider {
    fn id(&self) -> &str {
        "chrome"
    }

    fn name(&self) -> &str {
        "Chromium"
    }

    fn group_name(&self) -> Option<&str> {
        None
    }

    async fn generate(
        &self,
        _previous: Option<ProviderCacheMultiple>,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> anyhow::Result<ProviderCache> {
        trace!("Downloading translation expectations...");

        let url = "https://chromium.googlesource.com/chromium/src/+/main/tools/gritsettings/translation_expectations.pyl?format=TEXT";
        let pyl = download(url, "translation expectations", &client).await?;
        let pyl = Regex::new("#.*\n").unwrap().replace_all(&pyl, "");
        let pyl = Regex::new(r",\s*}").unwrap().replace_all(&pyl, "}");
        let pyl = Regex::new(r",\s*]").unwrap().replace_all(&pyl, "]");
        let translation_expectations: TranslationExpectations = serde_json::from_str(&pyl)
            .map_err(|e| anyhow!("Could not parse translation expectations: {e}\n{url}"))?;

        let mut grits = Vec::new();

        for translations in translation_expectations.translations.into_values() {
            if !lang_ids
                .iter()
                .any(|lang_id| translations.languages.contains(lang_id))
            {
                continue;
            }
            for path in translations.files {
                let url = format!(
                    "https://chromium.googlesource.com/chromium/src/+/main/{path}?format=TEXT"
                );
                let xml = download(&url, "Grit file", &client).await?;
                let grit: Grit = quick_xml::de::from_str(&xml)
                    .map_err(|e| anyhow!("Could not parse Grit file: {e}\n{url}"))?;
                grits.push((path, grit));
            }
        }

        trace!("Downloaded {} Grit files", grits.len());

        let mut translation_bundle: TranslationBundle = BTreeMap::new();

        for (path, grit) in &grits {
            let Some(grit_en) = grit
                .translations
                .file
                .iter()
                .find(|grit| grit.lang == "en-GB")
            else {
                continue;
            };
            let url_en = format!(
                "https://chromium.googlesource.com/chromium/src/+/main/{}/{}?format=TEXT",
                path.rsplit_once('/').map(|(path, _)| path).unwrap_or(""),
                grit_en.path
            );
            let xml_en = download(&url_en, "English messages", &client).await?;
            let messages_en = parse_grit(&xml_en)
                .map_err(|e| anyhow!("Could not parse English messages: {e}\n{url_en}"))?;

            for lang_id in &lang_ids {
                let Some(translation) = grit
                    .translations
                    .file
                    .iter()
                    .find(|grit| grit.lang == *lang_id)
                else {
                    continue;
                };

                let url = format!(
                    "https://chromium.googlesource.com/chromium/src/+/main/{}/{}?format=TEXT",
                    path.rsplit_once('/').map(|(path, _)| path).unwrap_or(""),
                    translation.path
                );
                let xml = download(&url, "translation bundle", &client).await?;
                let messages = parse_grit(&xml)
                    .map_err(|e| anyhow!("Could not parse messages: {e}\n{url}"))?;

                let translations = translation_bundle
                    .entry(lang_id.clone())
                    .and_modify(|entry| {
                        if entry.is_none() {
                            *entry = Some(Vec::new())
                        }
                    })
                    .or_insert_with(|| Some(Vec::new()))
                    .as_mut()
                    .unwrap();

                for (id, translation) in messages {
                    let Some(translation_en) = messages_en.get(&id) else {
                        continue;
                    };

                    translations.push(Translation {
                        original: unescape(translation_en, &['n', 'u']),
                        translation: unescape(&translation, &['n', 'u']),
                        comment: None,
                        key: Some(id.to_string()),
                        source: url.clone(),
                    })
                }
            }
        }

        for lang_id in lang_ids {
            translation_bundle.entry(lang_id).or_insert(None);
        }

        Ok(ProviderCache::Single(translation_bundle))
    }
}

async fn download(url: &str, error_file_name: &str, client: &Client) -> anyhow::Result<String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("Could not download {error_file_name}: {e}\n{url}"))?;
    if response.status() != StatusCode::OK {
        bail!("Unexpected status code: {}\n{url}", response.status());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| anyhow!("Could not download {error_file_name} bytes: {e}\n{url}"))?;
    let bytes = BASE64.decode(&bytes).map_err(|e| {
        anyhow!(
            "Could not parse {error_file_name} as base64: {e}\n{}",
            String::from_utf8_lossy(&bytes)
        )
    })?;
    let xml = String::from_utf8(bytes)
        .map_err(|e| anyhow!("Could not parse {error_file_name} as string: {e}"))?;

    Ok(xml)
}

fn parse_grit(xml: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut messages = HashMap::new();

    let mut reader = Reader::from_str(xml);
    let mut key: Option<String> = None;
    let mut message = String::new();
    loop {
        match reader.read_event()? {
            Event::Eof => break,
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
            Event::End(e) if e.name().as_ref() == b"translation" => {
                if let Some(id) = key {
                    messages.insert(id, message);
                }

                key = None;
                message = String::new();
            }

            Event::Start(e) if e.name().as_ref() == b"translationbundle" => {}
            Event::End(e) if e.name().as_ref() == b"translationbundle" => {}

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

#[derive(Debug, Deserialize, Serialize)]
struct TranslationExpectations {
    untranslated_grds: HashMap<String, String>,
    internal_grds: Vec<String>,

    #[serde(flatten)]
    translations: HashMap<String, TranslationExpectationsTranslations>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TranslationExpectationsTranslations {
    languages: Vec<LanguageIdentifier>,
    files: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Grit {
    translations: GritTranslations,
}

#[derive(Debug, Deserialize, Serialize)]
struct GritTranslations {
    file: Vec<GritFile>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GritFile {
    #[serde(rename = "@path")]
    path: String,
    #[serde(rename = "@lang")]
    lang: LanguageIdentifier,
}
