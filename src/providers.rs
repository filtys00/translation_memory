// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod android;
mod browser_extension;
mod chrome;
mod defaults;
mod dtd;
mod eu;
mod json;
mod microsoft;
mod minecraft;
mod mozilla;
mod po;
mod properties;
mod srt;
mod ts;
mod yaml;

pub use defaults::{default_providers, simple_provider, SimpleProvider};

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use log::trace;
use reqwest::{Client, StatusCode};
use unic_langid::LanguageIdentifier;

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

/// Returns `text` where all `allowed_escapes` have been resolved.
///
/// Escapes are a backslash (`\`) followed by an `allowed_escape` character.
/// Any escape with a character that is not in `allowed_escape` will be ignored.
///
/// Special cases:
/// - `\n` -> a new line
/// - `\uxxxx` -> an Unicode code point
/// - `\t`-> a tab
fn unescape(text: &str, allowed_escapes: &[char]) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }

    let mut new_text = String::with_capacity(text.len());
    let mut escape = false;
    let mut skip = 0;
    for (i, c) in text.char_indices() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if escape {
            escape = false;
            if !allowed_escapes.contains(&c) {
                new_text.push('\\');
                new_text.push(c);
                continue;
            }
            match c {
                'n' => new_text.push('\n'),
                't' => new_text.push('\t'),
                'u' => {
                    if let Some(hex) = text.get((i + 1)..(i + 5)) {
                        if let Ok(point) = u32::from_str_radix(hex, 16) {
                            if let Some(c) = char::from_u32(point) {
                                new_text.push(c);
                                skip = 4;
                                continue;
                            }
                        }
                    }
                    new_text.push_str("\\u");
                }
                c => new_text.push(c),
            }
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        new_text.push(c);
    }

    new_text
}

/// Key-value map containing the translation messages of a single language.
///
/// Use `merge_messages` to convert to a list of `Translation`s.
type TranslationMessages = HashMap<String, (String, Option<String>)>;

/// Returns a vector of `Translation`s by merging the translated `messages` and the English `messages_en`.
///
/// `source` is used to set `Translation.source` on all the returned `Translation`s.
pub fn merge_messages(
    messages: TranslationMessages,
    messages_en: &TranslationMessages,
    source: &str,
) -> Vec<Translation> {
    let mut translations = Vec::with_capacity(messages.len());

    for (key, (translation, comment)) in messages {
        let Some((original, comment_en)) = messages_en.get(&key) else {
            continue;
        };
        translations.push(Translation {
            original: original.clone(),
            translation,
            comment: comment.or_else(|| comment_en.as_ref().cloned()),
            key: Some(key),
            source: source.to_string(),
        });
    }

    translations
}

/// Standard provider for translation formats where both the original strings
/// and the translations are located within the same file.
pub struct MonoProvider<U, P>
where
    U: Fn(&LanguageIdentifier) -> String + Send + Sync,
    P: Fn(String, &str) -> anyhow::Result<Vec<Translation>> + Send + Sync,
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
    P: Fn(String, &str) -> anyhow::Result<Vec<Translation>> + Send + Sync,
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
                let mut t = (self.parse)(text, &url)?;
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

            translations.insert(lang_id, Some(merge_messages(messages, &messages_en, &url)));
        }

        Ok(translations)
    }
}
