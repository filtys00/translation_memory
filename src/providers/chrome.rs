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

use crate::{Translation, TranslationProvider};

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
        "Google Chrome"
    }

    fn group_name(&self) -> Option<&str> {
        None
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error> {
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

        let mut translations: BTreeMap<LanguageIdentifier, Option<Vec<Translation>>> =
            BTreeMap::new();

        for (path, grit) in &grits {
            let Some(translation_en) = grit
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
                translation_en.path
            );
            let xml_en = download(&url_en, "English translation bundle", &client).await?;
            let translation_bundle_en = parse_translations(&xml_en).map_err(|e| {
                anyhow!("Could not parse English translation bundle: {e}\n{url_en}")
            })?;

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
                let translation_bundle = parse_translations(&xml)
                    .map_err(|e| anyhow!("Could not parse translation bundle: {e}\n{url}"))?;

                let translations = translations
                    .entry(lang_id.clone())
                    .and_modify(|entry| {
                        if entry.is_none() {
                            *entry = Some(Vec::new())
                        }
                    })
                    .or_insert_with(|| Some(Vec::new()))
                    .as_mut()
                    .unwrap();

                for (id, translation) in translation_bundle {
                    let Some(translation_en) = translation_bundle_en.get(&id) else {
                        continue;
                    };

                    translations.push(Translation {
                        original: translation_en.clone(),
                        translation,
                        comment: None,
                    })
                }
            }
        }

        for lang_id in lang_ids {
            translations.entry(lang_id).or_insert(None);
        }

        Ok(translations)
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

fn parse_translations(xml: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut translations = HashMap::new();

    let mut reader = Reader::from_str(xml);
    let mut id: Option<String> = None;
    let mut text = String::new();
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
                id = Some(String::from_utf8(attribute.value.to_vec())?);
                text.clear();
            }
            Event::Text(bytes) => text.push_str(&String::from_utf8(bytes.to_vec())?),
            Event::Empty(e) if e.name().as_ref() == b"ph" => {
                let Some(attribute) = e
                    .attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"name")
                else {
                    continue;
                };
                text.push_str(&format!(
                    "<ph name=\"{}\" />",
                    String::from_utf8(attribute.value.to_vec())?
                ))
            }
            Event::End(e) if e.name().as_ref() == b"translation" => {
                if let Some(id) = id {
                    translations.insert(id, text);
                }

                id = None;
                text = String::new();
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

    Ok(translations)
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
