// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub mod builtin;

mod android;
mod browser_extension;
mod chrome;
mod dtd;
mod eu;
mod gnome;
mod json;
mod kde;
mod libreoffice;
mod microsoft;
mod minecraft;
mod mozilla;
mod po;
mod properties;
mod srt;
mod ts;
mod yaml;

use std::{collections::{BTreeMap, HashMap}, future::Future, iter};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use log::{debug, error, trace};
use reqwest::{Client, StatusCode, Url};
use tokio::task::JoinSet;
use unic_langid::LanguageIdentifier;

#[derive(Clone, Debug)]
pub struct Translation {
    pub original: String,
    pub translation: String,
    pub comment: Option<String>,
    pub key: Option<String>,
    pub source: String,
}

pub type TranslationBundle = BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>;

#[derive(Debug)]
pub enum ProviderCache {
    Single(TranslationBundle),
    Multiple(ProviderCacheMultiple),
}

#[derive(Debug)]
pub struct ProviderCacheMultiple {
    /// If all available translation bundles have been added to `translation_bundles`.
    pub finished: bool,
    pub translation_bundles: BTreeMap<String, TranslationBundle>,
}

impl ProviderCache {
    /// Returns an iterator over the cached translation bundles.
    pub fn translation_bundles(&self) -> Box<dyn Iterator<Item = &TranslationBundle> + '_> {
        match self {
            Self::Single(translation_bundle) => Box::new(iter::once(translation_bundle)),
            Self::Multiple(multiple) => Box::new(multiple.translation_bundles.values()),
        }
    }

    /// Returns a mutable iterator over the cached translation bundles.
    pub fn translation_bundles_mut(
        &mut self,
    ) -> Box<dyn Iterator<Item = &mut TranslationBundle> + '_> {
        match self {
            Self::Single(translation_bundle) => Box::new(iter::once(translation_bundle)),
            Self::Multiple(multiple) => Box::new(multiple.translation_bundles.values_mut()),
        }
    }
}

/// A function that parses a translation file.
pub enum SimpleProvider {
    Mono(fn(String, &str) -> anyhow::Result<Vec<Translation>>),
    Duo(fn(String) -> anyhow::Result<TranslationMessages>),
}

/// Returns a `SimpleProvider` corresponding with `type_name`.
#[rustfmt::skip]
pub fn simple_provider(type_name: &str) -> Option<SimpleProvider> {
    match type_name {
               "androidxml" => Some(SimpleProvider::Duo(android::parse_android)),
        "browser_extension" => Some(SimpleProvider::Duo(browser_extension::parse_browser_extension)),
             "microsofttbx" => Some(SimpleProvider::Mono(microsoft::parse_microsoft_tbx)),
                       "po" => Some(SimpleProvider::Mono(po::parse_po)),
               "properties" => Some(SimpleProvider::Duo(properties::parse_properties)),
        _ => None,
    }
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
                    let c = text.get((i + 1)..(i + 5))
                        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                        .and_then(char::from_u32);
                    if let Some(c) = c {
                        new_text.push(c);
                        skip = 4;
                        continue;
                    } else {
                        new_text.push_str("\\u");
                    }
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

    /// Returns a `ProviderCache` with translations for the languages in `lang_ids`.
    ///
    /// If this function returns `ProviderCache::Multiple(multiple)` with `multiple.finished` set to `false`,
    /// then `multiple` is given back in `previous` the next time `generate` is invoced.
    async fn generate(
        &self,
        previous: Option<ProviderCacheMultiple>,
        lang_ids: Vec<LanguageIdentifier>,
        client: Client,
    ) -> anyhow::Result<ProviderCache>;
}

/// Standard provider for translation formats where both the original strings
/// and the translations are located within the same file.
struct MonoProvider<U, P>
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
        _previous: Option<ProviderCacheMultiple>,
        lang_ids: Vec<LanguageIdentifier>,
        client: Client,
    ) -> anyhow::Result<ProviderCache> {
        let mut translation_bundle = BTreeMap::new();

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let text = download_text(&url, &client).await?;

            if let Some(text) = text {
                let mut translations = (self.parse)(text, &url)?;
                if let Some(remove_char) = &self.remove_char {
                    translations.iter_mut().for_each(|translation| {
                        translation.original = translation.original.replace(*remove_char, "");
                        translation.translation = translation.translation.replace(*remove_char, "");
                    });
                }
                translation_bundle.insert(lang_id, Some(translations));
            } else {
                translation_bundle.insert(lang_id, None);
            }
        }

        Ok(ProviderCache::Single(translation_bundle))
    }
}

/// Standard provider for translation formats where the original strings
/// and the translations are located in separate files.
struct DuoProvider<U, P>
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
        _previous: Option<ProviderCacheMultiple>,
        lang_ids: Vec<LanguageIdentifier>,
        client: Client,
    ) -> anyhow::Result<ProviderCache> {
        let mut translation_bundle = BTreeMap::new();

        let text_en = download_text(self.default_url, &client)
            .await?
            .ok_or_else(|| anyhow!("Default translation were not found\n{}", self.default_url))?;
        let messages_en = (self.parse)(text_en)?;

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let Some(text) = download_text(&url, &client).await? else {
                translation_bundle.insert(lang_id, None);
                continue;
            };
            let messages = (self.parse)(text)?;
            let translations = merge_messages(messages, &messages_en, &url);

            translation_bundle.insert(lang_id, Some(translations));
        }

        Ok(ProviderCache::Single(translation_bundle))
    }
}

/// Append the translation bundles from `join_set` into `multiple`.
async fn append_to_multiple(
    multiple: &mut ProviderCacheMultiple,
    provider_id: &str,
    mut join_set: JoinSet<anyhow::Result<(String, TranslationBundle)>>,
) {
    let mut failed = 0;

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok((bundle_id, translation_bundle))) => {
                multiple
                    .translation_bundles
                    .insert(bundle_id, translation_bundle);
            }
            Ok(Err(e)) => {
                error!("Could not generate translation bundle: {e}");
                failed += 1;
                continue;
            }
            Err(e) => {
                error!("Could not request translation bundle: {e}");
                failed += 1;
                continue;
            }
        }
    }

    if failed > 0 {
        debug!("Failed to generate '{failed}' translation bundles for '{provider_id}'");
        multiple.finished = false;
    } else {
        multiple.finished = true;
    }

    join_set.abort_all();
}

/// Standard provider for single-file translation formats,
/// that are downloaded from a dynamically generated list of URLs.
struct MassMonoProvider<U, F>
where
    U: Fn(Vec<LanguageIdentifier>, Client) -> F + Send + Sync + 'static,
    F: Future<Output = anyhow::Result<HashMap<String, HashMap<LanguageIdentifier, Option<Url>>>>>
        + Send
        + Sync
        + 'static,
{
    pub id: &'static str,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub urls: U,
    pub parse: fn(String, &str) -> anyhow::Result<Vec<Translation>>,
    pub remove_char: Option<char>,
}

#[async_trait]
impl<U, F> TranslationProvider for MassMonoProvider<U, F>
where
    U: Fn(Vec<LanguageIdentifier>, Client) -> F + Send + Sync + 'static,
    F: Future<Output = anyhow::Result<HashMap<String, HashMap<LanguageIdentifier, Option<Url>>>>>
        + Send
        + Sync
        + 'static,
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
        previous: Option<ProviderCacheMultiple>,
        lang_ids: Vec<LanguageIdentifier>,
        client: Client,
    ) -> anyhow::Result<ProviderCache> {
        let mut multiple = previous.unwrap_or_else(|| ProviderCacheMultiple {
            finished: false,
            translation_bundles: BTreeMap::new(),
        });

        let urls = match (self.urls)(lang_ids, client.clone()).await {
            Ok(urls) => urls,
            Err(e) => {
                error!("Could not get translation URLs for '{}': {e}", self.id,);
                return Ok(ProviderCache::Multiple(multiple));
            }
        };

        trace!(
            "Got {} translation bundles with {} URLs for '{}'",
            urls.len(),
            urls.values()
                .flat_map(|bundle| bundle.values())
                .filter_map(|translations| translations.as_ref())
                .count(),
            self.id
        );

        let mut join_set: JoinSet<anyhow::Result<(String, TranslationBundle)>> = JoinSet::new();

        for (bundle_id, urls) in urls {
            if multiple.translation_bundles.contains_key(&bundle_id) {
                continue;
            }

            let client = client.clone();
            let parse = self.parse;
            let remove_char = self.remove_char;

            join_set.spawn(async move {
                let mut translation_bundle = TranslationBundle::new();

                for (lang_id, url) in urls {
                    let Some(url) = url else {
                        translation_bundle.insert(lang_id, None);
                        continue;
                    };
                    let Some(text) = download_text(url.as_str(), &client).await? else {
                        translation_bundle.insert(lang_id, None);
                        continue;
                    };
                    let mut translations = parse(text, url.as_str())?;

                    if let Some(remove_char) = remove_char {
                        translations.iter_mut().for_each(|translation| {
                            translation.original = translation.original.replace(remove_char, "");
                            translation.translation =
                                translation.translation.replace(remove_char, "");
                        });
                    }

                    translation_bundle.insert(lang_id, Some(translations));
                }

                Ok((bundle_id, translation_bundle))
            });
        }

        append_to_multiple(&mut multiple, self.id, join_set).await;

        Ok(ProviderCache::Multiple(multiple))
    }
}

/// Adapt a function `urls`, that returns a list of URLs for one language,
/// to return a format that is accepted by `MassMonoProvider`.
async fn adapt_urls_to_mass<F>(
    urls: fn(LanguageIdentifier, Client) -> F,
    lang_ids: Vec<LanguageIdentifier>,
    client: Client,
) -> anyhow::Result<HashMap<String, HashMap<LanguageIdentifier, Option<Url>>>>
where
    F: Future<Output = anyhow::Result<Vec<String>>> + Send + Sync + 'static,
{
    let mut url_bundles = HashMap::new();

    let mut join_set = JoinSet::new();
    for lang_id in lang_ids {
        let client = client.clone();

        join_set.spawn(async move {
            let urls = urls(lang_id.clone(), client.clone()).await?;

            let mut url_bundles = Vec::new();
            for url in urls {
                let mut url_bundle = HashMap::with_capacity(1);
                url_bundle.insert(lang_id.clone(), Some(Url::parse(&url)?));
                url_bundles.push((url, url_bundle));
            }

            Ok((lang_id, url_bundles))
        });
    }

    let mut none_lang_ids = Vec::new();

    while let Some(result) = join_set.join_next().await {
        let result: anyhow::Result<_> = result?;
        let (lang_id, lang_url_bundles) = result?;

        if lang_url_bundles.is_empty() {
            none_lang_ids.push(lang_id);
        } else {
            url_bundles.extend(lang_url_bundles);
        }
    }

    for url_bundle in url_bundles.values_mut() {
        for lang_id in &none_lang_ids {
            url_bundle.entry(lang_id.clone()).or_insert(None);
        }
    }

    Ok(url_bundles)
}

/// Standard provider for split-file translation formats,
/// that are downloaded from a dynamically generated list of URLs.
struct MassDuoProvider<U, F>
where
    U: Fn(Vec<LanguageIdentifier>, Client) -> F + Send + Sync + 'static,
    F: Future<
            Output = anyhow::Result<
                HashMap<String, (Url, HashMap<LanguageIdentifier, Option<Url>>)>,
            >,
        > + Send
        + Sync
        + 'static,
{
    pub id: &'static str,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub urls: U,
    pub parse: fn(String) -> anyhow::Result<TranslationMessages>,
}

#[async_trait]
impl<U, F> TranslationProvider for MassDuoProvider<U, F>
where
    U: Fn(Vec<LanguageIdentifier>, Client) -> F + Send + Sync + 'static,
    F: Future<
            Output = anyhow::Result<
                HashMap<String, (Url, HashMap<LanguageIdentifier, Option<Url>>)>,
            >,
        > + Send
        + Sync
        + 'static,
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
        previous: Option<ProviderCacheMultiple>,
        lang_ids: Vec<LanguageIdentifier>,
        client: Client,
    ) -> anyhow::Result<ProviderCache> {
        let mut multiple = previous.unwrap_or_else(|| ProviderCacheMultiple {
            finished: false,
            translation_bundles: BTreeMap::new(),
        });

        let urls = match (self.urls)(lang_ids, client.clone()).await {
            Ok(urls) => urls,
            Err(e) => {
                error!("Could not get translation URLs for '{}': {e}", self.id);
                return Ok(ProviderCache::Multiple(multiple));
            }
        };

        trace!(
            "Got {} translation bundles with {} URLs for '{}'",
            urls.len(),
            urls.values()
                .flat_map(|(_, bundle)| bundle.values())
                .filter_map(|translations| translations.as_ref())
                .count(),
            self.id
        );

        let mut join_set: JoinSet<anyhow::Result<(String, TranslationBundle)>> = JoinSet::new();

        for (bundle_id, (default_url, urls)) in urls {
            if multiple.translation_bundles.contains_key(&bundle_id) {
                continue;
            }

            let client = client.clone();
            let parse = self.parse;

            join_set.spawn(async move {
                let mut translation_bundle = TranslationBundle::new();

                let Some(text_en) = download_text(default_url.as_str(), &client).await? else {
                    bail!("Default translation were not found\n{default_url}");
                };
                let messages_en = parse(text_en)?;

                for (lang_id, url) in urls {
                    let Some(url) = url else {
                        translation_bundle.insert(lang_id, None);
                        continue;
                    };
                    let Some(text) = download_text(url.as_str(), &client).await? else {
                        translation_bundle.insert(lang_id, None);
                        continue;
                    };
                    let messages = parse(text)?;
                    let translations = merge_messages(messages, &messages_en, url.as_str());

                    translation_bundle.insert(lang_id, Some(translations));
                }

                Ok((bundle_id, translation_bundle))
            });
        }

        append_to_multiple(&mut multiple, self.id, join_set).await;

        Ok(ProviderCache::Multiple(multiple))
    }
}
