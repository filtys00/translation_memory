pub mod gnome;
pub mod kde;

use std::{
    collections::{BTreeMap, HashMap},
    env,
    fs::{self, File},
    future::Future,
    io::{BufWriter, Write},
    sync::Arc,
};

use anyhow::anyhow;
use async_trait::async_trait;
use log::{debug, error};
use polib::po_file::{self};
use reqwest::Client;
use tokio::task::JoinSet;
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

pub struct PoProvider<F, U>
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
impl<F, U> TranslationProvider for PoProvider<F, U>
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

        let mut join_set: JoinSet<anyhow::Result<(LanguageIdentifier, Vec<Translation>)>> =
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

                Ok((lang_id, translations))
            });
        }

        while let Some(result) = join_set.join_next().await {
            let (lang_id, t) = result??;
            translations.insert(lang_id, Some(t));
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
    let path = env::temp_dir().join(format!(
        "{}_{}.po",
        env!("CARGO_PKG_NAME"),
        url.split('/').skip(3).fold(String::new(), |mut acc, part| {
            acc.push('_');
            acc.push_str(part);
            acc
        })
    ));

    let file = File::create(&path).map_err(|e| anyhow!("Could not create file {path:?}: {e}"))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(&response.bytes().await?)
        .map_err(|e| anyhow!("Could not write to file {path:?}: {e}"))?;
    let po = po_file::parse(&path)?;
    fs::remove_file(&path).map_err(|e| anyhow!("Could not delete file {path:?}: {e}"))?;

    let mut translations = Vec::with_capacity(po.count());
    for message in po.messages() {
        if !message.is_translated() {
            continue;
        }
        let msgstr = match message.msgstr() {
            Ok(msgstr) => msgstr,
            Err(_) => {
                continue;
            }
        };
        translations.push(Translation {
            original: if let Some(remove_char) = remove_char {
                message.msgid().replace(remove_char, "")
            } else {
                message.msgid().to_string()
            },
            translation: if let Some(remove_char) = remove_char {
                msgstr.replace(remove_char, "")
            } else {
                msgstr.to_string()
            },
            comment: if message.comments().is_empty() {
                None
            } else {
                Some(message.comments().trim().to_string())
            },
        });
    }
    Ok(translations)
}
