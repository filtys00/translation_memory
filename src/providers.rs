// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod android;
mod browser_extension;
mod chrome;
mod defaults;
mod dtd;
mod json;
mod microsoft;
mod minecraft;
mod mozilla;
mod po;
mod properties;
mod srt;
mod yaml;

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use log::trace;
use reqwest::{Client, StatusCode};
use unic_langid::LanguageIdentifier;

pub use self::{
    android::parse_android, defaults::default_providers, microsoft::parse_microsoft_tbx,
};
use super::Translation;

#[async_trait]
pub trait TranslationProvider {
    fn id(&self) -> &str;

    fn name(&self) -> &str;

    fn group_name(&self) -> Option<&str> {
        None
    }

    /// Returns `true` if associated data should not be saved to disk.
    fn temporary(&self) -> bool {
        false
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error>;
}

/// Returns a string version of `lang_id`.
///
/// # Examples
/// ```ignore
/// assert!(
///     lang_id_to_string("ca_ES_valencia".parse().unwrap(), "-", false, "@", true),
///     String::from("ca-es@VALENCIA"),
/// );
/// ```
fn lang_id_to_string(
    lang_id: &LanguageIdentifier,
    region_binder: &str,
    uppercase_region: bool,
    variant_binder: &str,
    uppercase_variant: bool,
) -> String {
    let mut s = lang_id.language.to_string();
    if let Some(region) = &lang_id.region {
        s.push_str(region_binder);
        if uppercase_region {
            s.push_str(region.as_str());
        } else {
            s.push_str(&region.as_str().to_lowercase());
        }
    }
    for variant in lang_id.variants() {
        s.push_str(variant_binder);
        if uppercase_variant {
            s.push_str(&variant.as_str().to_uppercase());
        } else {
            s.push_str(variant.as_str());
        }
    }
    s
}

/// Returns the contents of `url` as text.
async fn download_text(url: &str, client: &Client) -> anyhow::Result<Option<String>> {
    trace!("Sending request: {url}");
    let response = client.get(url).send().await?;
    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return Ok(None),
        status_code => bail!("Unexpected status code: {status_code}\n{url}"),
    }
    let text = response.text().await?;
    Ok(Some(text))
}

/// Key-value map containing the translation messages of a single language.
///
/// Use `merge_messages` to convert to a list of `Translation`s.
type TranslationMessages = HashMap<String, (String, Option<String>)>;

/// Returns a vector of `Translation`s by merging the translated `messages` and the English `messages_en`.
pub fn merge_messages(
    messages: TranslationMessages,
    messages_en: &TranslationMessages,
) -> Vec<Translation> {
    let mut translations = Vec::with_capacity(messages.len());

    for (key, (translation, comment)) in messages {
        let Some((original, comment_en)) = messages_en.get(&key) else {
            continue;
        };
        let comment = match (comment_en, comment) {
            (Some(comment), _) => format!("Key: {key}\n{comment}"),
            (None, Some(comment)) => format!("Key: {key}\n{comment}"),
            (None, None) => format!("Key: {key}"),
        };
        translations.push(Translation {
            original: original.clone(),
            translation,
            comment: Some(comment),
        });
    }

    translations
}

/// Standard provider for translation formats where both the original strings
/// and the translations are located within the same file.
pub struct MonoProvider<U, P>
where
    U: Fn(&LanguageIdentifier) -> String + Send + Sync,
    P: Fn(String) -> anyhow::Result<Vec<Translation>> + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub parse: P,
    pub remove_char: Option<char>,
    pub group_name: Option<&'static str>,
    pub url: U,
}

#[async_trait]
impl<U, P> TranslationProvider for MonoProvider<U, P>
where
    U: Fn(&LanguageIdentifier) -> String + Send + Sync,
    P: Fn(String) -> anyhow::Result<Vec<Translation>> + Send + Sync,
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
            let text = download_text(&url, &client).await?;

            if let Some(text) = text {
                let mut t = (self.parse)(text)?;
                if let Some(remove_char) = &self.remove_char {
                    t.iter_mut().for_each(|translation| {
                        translation.original = translation.original.replace(*remove_char, "");
                        translation.translation = translation.translation.replace(*remove_char, "");
                    });
                }
                translations.insert(lang_id, Some(t));
            } else {
                translations.insert(lang_id, None);
            }
        }

        Ok(translations)
    }
}

/// Standard provider for translation formats where the original strings
/// and the translations are located in separate files.
pub struct DuoProvider<U, P>
where
    U: Fn(&LanguageIdentifier) -> String + Send + Sync,
    P: Fn(String) -> anyhow::Result<TranslationMessages> + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub parse: P,
    pub default_url: &'static str,
    pub url: U,
}

#[async_trait]
impl<U, P> TranslationProvider for DuoProvider<U, P>
where
    U: Fn(&LanguageIdentifier) -> String + Send + Sync,
    P: Fn(String) -> anyhow::Result<TranslationMessages> + Send + Sync,
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

        let text_en = download_text(self.default_url, &client)
            .await?
            .ok_or_else(|| anyhow!("Default translation were not found\n{}", self.default_url))?;
        let messages_en = (self.parse)(text_en)?;

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let Some(text) = download_text(&url, &client).await? else {
                translations.insert(lang_id, None);
                continue;
            };
            let messages = (self.parse)(text)?;

            translations.insert(lang_id, Some(merge_messages(messages, &messages_en)));
        }

        Ok(translations)
    }
}
