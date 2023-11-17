use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    fs::{self, File},
    io::{BufWriter, Write},
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use polib::po_file::{self};
use reqwest::{Client, StatusCode};
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

pub struct PoHttpProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    pub id: Cow<'static, str>,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub check_url: Cow<'static, str>,
    pub url: F,
    pub remove_char: char,
}

#[async_trait]
impl<F> TranslationProvider for PoHttpProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    fn id(&self) -> &str {
        &self.id
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
        client: &Client,
    ) -> anyhow::Result<HashMap<LanguageIdentifier, Option<Vec<Translation>>>> {
        let mut translations = HashMap::new();

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let response = client.get(&url).send().await?;
            if response.status() == StatusCode::NOT_FOUND {
                translations.insert(lang_id, None);
                continue;
            }
            let path = env::temp_dir().join(format!(
                "{}_{}_{lang_id}.po",
                env!("CARGO_PKG_NAME"),
                self.id,
            ));
            let file =
                File::create(&path).map_err(|e| anyhow!("Could not create file {path:?}: {e}"))?;
            let mut writer = BufWriter::new(file);
            writer
                .write_all(&response.bytes().await?)
                .map_err(|e| anyhow!("Could not write to file {path:?}: {e}"))?;
            let po = po_file::parse(&path)?;
            fs::remove_file(&path).map_err(|e| anyhow!("Could not delete file {path:?}: {e}"))?;

            let mut t = Vec::with_capacity(po.count());
            for message in po.messages() {
                if !message.is_translated() {
                    continue;
                }
                let Ok(msgstr) = message.msgstr() else {
                    continue;
                };
                t.push(Translation {
                    original: message.msgid().replace(self.remove_char, ""),
                    translation: msgstr.replace(self.remove_char, ""),
                    comment: if message.comments().is_empty() {
                        None
                    } else {
                        Some(message.comments().to_string())
                    },
                });
            }

            translations.insert(lang_id, Some(t));
        }

        if translations.is_empty() {
            let response = client.head(&*self.check_url).send().await?;
            if response.status() != StatusCode::OK {
                bail!("Invalid url");
            }
        }

        Ok(translations)
    }
}
