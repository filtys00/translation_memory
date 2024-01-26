// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{collections::BTreeMap, fmt::Display, sync::Arc};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use log::trace;
use reqwest::{Client, StatusCode};
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

pub struct SrtProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub default_url: &'static str,
    pub url: F,
}

#[async_trait]
impl<F> TranslationProvider for SrtProvider<F>
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

        let subtitles_en = get_single(self.default_url, "default", &client)
            .await?
            .ok_or_else(|| anyhow!("Could not find default subtitles\n{}", self.default_url))?;
        log::error!("{subtitles_en:#?}");

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let Some(subtitles) = get_single(&url, &lang_id, &client).await? else {
                translations.insert(lang_id, None);
                continue;
            };
            log::error!("{subtitles:#?}");

            if subtitles.len() != subtitles_en.len() {
                bail!(
                    "Different subtitle amounts: default {}, '{lang_id}' {}",
                    subtitles_en.len(),
                    subtitles.len()
                );
            }
            let t = subtitles_en
                .iter()
                .zip(subtitles.into_iter())
                .map(|(subtitle_en, subtitle)| Translation {
                    original: subtitle_en.clone(),
                    translation: subtitle,
                    comment: None,
                })
                .collect();
            translations.insert(lang_id, Some(t));
        }

        Ok(translations)
    }
}

async fn get_single(
    url: &str,
    lang_id: impl Display,
    client: &Client,
) -> anyhow::Result<Option<Vec<String>>> {
    trace!("Requesting subtitle file for '{lang_id}'\n{url}");
    let response = client.get(url).send().await?;
    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return Ok(None),
        status_code => bail!("Unexpected status code: {status_code}\n{url}"),
    }
    let text = response.text().await?;

    let mut subtitles = Vec::new();
    let mut current_subtitle = String::new();
    let mut skip = 2;
    for line in text.lines() {
        if skip > 0 {
            if !line.is_empty() {
                skip -= 1;
            }
            continue;
        }

        if line.is_empty() {
            subtitles.push(current_subtitle);
            current_subtitle = String::new();
            skip = 2;
            continue;
        }

        if !current_subtitle.is_empty() {
            current_subtitle.push('\n');
        }
        current_subtitle.push_str(line);
    }
    if !current_subtitle.is_empty() {
        subtitles.push(current_subtitle);
    }

    Ok(Some(subtitles))
}
