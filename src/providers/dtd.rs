// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    sync::Arc,
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

pub struct DtdProvider<F>
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
impl<F> TranslationProvider for DtdProvider<F>
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
        let mut translations_all = BTreeMap::new();

        let resources_en = get_messages(
            self.default_lang,
            &(self.url)(&self.default_lang.parse()?),
            &client,
        )
        .await?
        .ok_or_else(|| anyhow!("Default language were not found: {}", self.default_lang))?;

        for lang_id in lang_ids {
            let Some(resources) = get_messages(&lang_id, &(self.url)(&lang_id), &client).await?
            else {
                translations_all.insert(lang_id, None);
                continue;
            };
            let mut translations = Vec::with_capacity(resources.len());
            for (name, (text, _)) in resources {
                let Some((text_en, comment_en)) = resources_en.get(&name) else {
                    continue;
                };
                translations.push(Translation {
                    original: text_en.clone(),
                    translation: text,
                    comment: comment_en.clone(),
                })
            }
            translations_all.insert(lang_id, Some(translations));
        }

        Ok(translations_all)
    }
}

async fn get_messages(
    lang_id: impl Display,
    url: &str,
    client: &Client,
) -> Result<Option<HashMap<String, (String, Option<String>)>>, anyhow::Error> {
    let response = client.get(url).send().await?;
    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return Ok(None),
        status_code => bail!("Translation '{lang_id}' returned error code '{status_code}'\n{url}",),
    }
    let text = response.text().await?;

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

        if let Some(line) = line.strip_prefix("<!ENTITY ") {
            if let Some(line) = line.strip_suffix('>') {
                if let Some((key, value)) = line.trim().split_once(' ') {
                    if messages.contains_key(key) {
                        bail!("Duplicate key: {key}");
                    }
                    if let Some(value) = value.trim_start().strip_prefix('"') {
                        if let Some(value) = value.trim_end().strip_suffix('"') {
                            let mut c = key.to_string();
                            if let Some(comment) = &comment.1 {
                                c.push('\n');
                                c.push_str(comment);
                            }
                            messages.insert(key.to_string(), (value.to_string(), Some(c)));
                            comment = (false, None);
                            continue;
                        }
                    }
                }
            }
        }

        bail!("Unexpected line: {line}");
    }

    Ok(Some(messages))
}
