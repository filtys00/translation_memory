// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub mod gnome;
pub mod kde;
pub mod libreoffice;

use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    iter,
    sync::Arc,
};

use anyhow::bail;
use async_trait::async_trait;
use log::{error, trace};
use reqwest::Client;
use tokio::task::JoinSet;
use unic_langid::LanguageIdentifier;

use super::{download_text, unescape};
use crate::{Translation, TranslationProvider};

pub struct NetPoProvider<F, U>
where
    F: Fn(LanguageIdentifier, Arc<Client>) -> U + Copy + Send + Sync + 'static,
    U: Future<Output = anyhow::Result<Vec<String>>> + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub urls: F,
    pub remove_char: Option<char>,
}

#[async_trait]
impl<F, U> TranslationProvider for NetPoProvider<F, U>
where
    F: Fn(LanguageIdentifier, Arc<Client>) -> U + Copy + Send + Sync + 'static,
    U: Future<Output = anyhow::Result<Vec<String>>> + Send + Sync,
{
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn group_name(&self) -> Option<&str> {
        None
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error> {
        let mut translations = BTreeMap::new();

        let mut join_set: JoinSet<anyhow::Result<(LanguageIdentifier, Option<Vec<Translation>>)>> =
            JoinSet::new();

        for lang_id in lang_ids {
            let id = self.id;
            let urls = self.urls;
            let remove_char = self.remove_char;
            let client = client.clone();

            join_set.spawn(async move {
                let urls = urls(lang_id.clone(), client.clone()).await?;

                trace!(
                    "Got {} translation URLs for '{lang_id}' from '{id}'",
                    urls.len(),
                );

                if urls.is_empty() {
                    return Ok((lang_id, None));
                }

                let mut translations = Vec::new();

                let mut join_set = JoinSet::new();
                for url in urls {
                    let client = client.clone();
                    join_set.spawn(async move {
                        let Some(text) = download_text(&url, &client).await? else {
                            bail!("not found\n{url}");
                        };
                        let mut translations = parse_po(text, &url)?;
                        if let Some(remove_char) = remove_char {
                            translations.iter_mut().for_each(|translation| {
                                translation.original =
                                    translation.original.replace(remove_char, "");
                                translation.translation =
                                    translation.translation.replace(remove_char, "");
                            });
                        }
                        Ok(translations)
                    });
                }

                while let Some(result) = join_set.join_next().await {
                    let mut result = match result {
                        Ok(Ok(result)) => result,
                        Ok(Err(e)) => {
                            error!("Could not get translation file: {e}");
                            continue;
                        }
                        Err(e) => {
                            error!("Could not request translation file: {e}");
                            continue;
                        }
                    };
                    translations.append(&mut result);
                }

                Ok((lang_id, Some(translations)))
            });
        }

        while let Some(result) = join_set.join_next().await {
            let (lang_id, t) = result??;
            translations.insert(lang_id, t);
        }

        join_set.abort_all();

        Ok(translations)
    }
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
