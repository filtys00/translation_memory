// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use log::warn;
use reqwest::{Client, StatusCode};
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

pub struct PropertiesProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub default_lang: &'static str,
    pub url: F,
}

#[async_trait]
impl<F> TranslationProvider for PropertiesProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn group_name(&self) -> Option<&str> {
        self.group_name
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error> {
        let mut translations = BTreeMap::new();

        let url = (self.url)(&self.default_lang.parse()?);
        let properties_en = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("{e}\n{url}"))?
            .text()
            .await
            .map_err(|e| anyhow!("{e}\n{url}"))?;
        let properties_en = parse_properties(&properties_en)?;

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(|e| anyhow!("{e}\n{url}"))?;
            match response.status() {
                StatusCode::OK => {}
                StatusCode::NOT_FOUND => continue,
                status_code => {
                    warn!("Unexpected status code: {status_code}\n{url}");
                    continue;
                }
            }
            let properties = response.text().await.map_err(|e| anyhow!("{e}\n{url}"))?;
            let properties = parse_properties(&properties)?;

            let mut t = Vec::with_capacity(properties.len());
            for (key, (property, _comment)) in properties {
                let Some((property_en, comment_en)) = properties_en.get(&key) else {
                    warn!(
                        "Translation key '{key}' were found in '{lang_id}' translation but not in '{}' translation",
                        self.default_lang,
                    );
                    continue;
                };

                t.push(Translation {
                    original: property_en.clone(),
                    translation: property,
                    comment: comment_en.clone(),
                });
            }
            translations.insert(lang_id, Some(t));
        }

        Ok(translations)
    }
}

fn parse_properties(file: &str) -> anyhow::Result<HashMap<String, (String, Option<String>)>> {
    let mut translations = HashMap::new();

    let mut comment = None;
    for mut line in file.split('\n') {
        if line.is_empty() {
            continue;
        }

        line = line.trim_start();
        if let Some(line) = line.strip_prefix('#') {
            comment = Some(line.trim_start());
            continue;
        }

        let Some((mut key, mut value)) = line.split_once('=') else {
            bail!("Invalid line: {line}");
        };
        key = key.trim_end();
        value = value.trim_start();

        if translations.contains_key(key) {
            bail!("Duplicate key: {key}");
        }
        translations.insert(
            key.to_string(),
            (
                value.to_string(),
                comment.map(|comment| comment.to_string()),
            ),
        );
    }

    Ok(translations)
}
