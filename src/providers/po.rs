pub mod gnome;
pub mod kde;

use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    sync::Arc,
};

use anyhow::bail;
use async_trait::async_trait;
use log::{debug, error, trace};
use reqwest::{Client, StatusCode};
use tokio::task::JoinSet;
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

pub struct PoProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub url: F,
    pub remove_char: Option<char>,
}

#[async_trait]
impl<F> TranslationProvider for PoProvider<F>
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
        None
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error> {
        let mut translations = BTreeMap::new();

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let translation = generate_single(url, self.remove_char, client.clone())
                .await
                .ok();
            translations.insert(lang_id, translation);
        }

        Ok(translations)
    }
}

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

                debug!(
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
                    join_set.spawn(generate_single(url, remove_char, client));
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

async fn generate_single(
    url: String,
    remove_char: Option<char>,
    client: Arc<Client>,
) -> anyhow::Result<Vec<Translation>> {
    let response = client.get(&url).send().await?;
    if response.status() != StatusCode::OK {
        bail!("Unexpected status code ({}): {url}", response.status());
    }
    let text = response.text().await?;

    let mut translations = Vec::new();

    let mut values: HashMap<&str, String> = HashMap::new();
    let mut last: Option<&mut String> = None;
    for line in text.lines() {
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
                    if let Some(remove_char) = remove_char {
                        escape_value(msgid).replace(remove_char, "")
                    } else {
                        escape_value(msgid)
                    }
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
                    if let Some(remove_char) = remove_char {
                        escape_value(msgstr).replace(remove_char, "")
                    } else {
                        escape_value(msgstr)
                    }
                },
                comment: values.remove("#.").or_else(|| values.remove("msgctxt")),
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

fn escape_value(value: String) -> String {
    value
        .replace("\\\\", "\u{0}")
        .replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace('\u{0}', "\\")
}
