// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use anyhow::bail;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

pub struct JsonProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub url: F,
}

#[async_trait]
impl<F> TranslationProvider for JsonProvider<F>
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

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let response = client.get(&url).send().await?;
            match response.status() {
                StatusCode::OK => {}
                StatusCode::NOT_FOUND => {
                    translations.insert(lang_id, None);
                    continue;
                }
                status => bail!("Unexpected status code: {status}\n{url}"),
            }
            let response: HashMap<String, String> = response.json().await?;

            let mut t: Vec<Translation> = Vec::new();
            for (original, translation) in response {
                if original.is_empty() || translation.is_empty() {
                    continue;
                }
                t.push(Translation {
                    original,
                    translation,
                    comment: None,
                });
            }
            translations.insert(lang_id, Some(t));
        }

        Ok(translations)
    }
}
