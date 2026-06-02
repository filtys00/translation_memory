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
mod xfce;
mod yaml;

use std::{cell::RefCell, collections::{HashMap, HashSet}, fmt::Display, ops::Deref, thread, time::Duration};

use anyhow::{bail};
use log::{debug, trace};
use reqwest::{blocking::Client, IntoUrl, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::Value as JsonValue;
use unic_langid::LanguageIdentifier;

use crate::database::{
    Provider as DbProvider,
    Source as DbSource,
    SourceContent as DbSourceContent,
    SourceContents as DbSourceContents,
    SourceUrls as DbSourceUrls,
    Translation as DbTranslation,
};

/// Wrapper for `unic_langid::LanguageIdentifier` with an added `format` method.
#[derive(PartialEq, Eq, Hash)]
pub struct LangId(LanguageIdentifier);

impl Deref for LangId {
    type Target = LanguageIdentifier;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl LangId {
    /// Format this language identifier as a string, with arguments.
    ///
    /// # Examples
    /// ```ignore
    /// assert!(
    ///     lang_id_to_string("ca_ES_valencia".parse().unwrap(), "-", false, "@", true),
    ///     String::from("ca-es@VALENCIA"),
    /// );
    /// ```
    pub fn format(&self, region_binder: &str, uppercase_region: bool, variant_binder: &str, uppercase_variant: bool) -> String {
        let mut s = self.language.to_string();
        if let Some(region) = &self.region {
            s.push_str(region_binder);
            if uppercase_region {
                s.push_str(region.as_str());
            } else {
                s.push_str(&region.as_str().to_lowercase());
            }
        }
        for variant in self.variants() {
            s.push_str(variant_binder);
            if uppercase_variant {
                s.push_str(&variant.as_str().to_uppercase());
            } else {
                s.push_str(variant.as_str());
            }
        }
        s
    }
}

/// Helper for downloading content from the internet.
pub struct Downloader {
    client: Client,
    /// Cache for only dowloading content once per URL.
    cache: RefCell<HashMap<Url, DbSourceContent>>,
    /// Map of timeouts per host.
    timeouts: RefCell<HashMap<String, Duration>>,
    /// The initial timout, timeout increase, and timeout decrease per download.
    default_timeout: (Duration, Duration, Duration),
    /// Maximum attemts at downloading and receiving a `HTTP 429 Too Many Requests` before giving up.
    max_attempts: u32,
}

impl Downloader {
    pub fn new() -> anyhow::Result<Self> {
        let client = Client::builder()
            .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
            .build()?; // Returns an error within an async runtime
        let downloader = Self {
            client,
            cache: RefCell::new(HashMap::new()),
            timeouts: RefCell::new(HashMap::new()),
            default_timeout: (Duration::from_secs(2), Duration::from_secs(2), Duration::from_secs_f32(0.1)),
            max_attempts: 3,
        };
        Ok(downloader)
    }

    /// Download `url` and return the content.
    pub fn get_content(&'_ self, url: impl IntoUrl) -> anyhow::Result<DbSourceContent> {
        let url = url.into_url()?;

        if let Some(content) = self.cache.borrow().get(&url) {
            return Ok(content.clone());
        }

        for _ in 0..self.max_attempts {
            if let Some(host) = url.host_str() && let Some(timeout) = self.timeouts.borrow_mut().get_mut(host) {
                thread::sleep(*timeout);
                *timeout = timeout.saturating_sub(self.default_timeout.2);
            }

            trace!("Sending request to '{url}'");
            let response = self.client.get(url.clone()).send()?;

            let content = match response.status() {
                StatusCode::NOT_FOUND => DbSourceContent::None,
                StatusCode::OK => {
                    let content_type = response.headers().get("content-type")
                        .and_then(|t| t.to_str().ok());

                    if content_type == Some("application/octet-stream") {
                        DbSourceContent::Bytes(response.bytes()?.to_vec())
                    } else {
                        DbSourceContent::Text(response.text()?)
                    }
                },
                StatusCode::TOO_MANY_REQUESTS => {
                    let retry_after = response.headers().get("retry-after")
                        .and_then(|r| r.to_str().ok())
                        .and_then(|r| r.parse().ok());

                    if let Some(retry_after) = retry_after {
                        trace!("Waiting for {retry_after}s due to 'Retry-After'");
                        let timeout = Duration::from_secs(retry_after);
                        let max_timeout = self.default_timeout.0 + self.default_timeout.1 * (self.max_attempts - 1);
                        if timeout > max_timeout {
                            bail!("'Retry-After' is too high at {retry_after}s (max is {}s)", max_timeout.as_secs_f32());
                        }
                        thread::sleep(timeout);
                    } else {
                        let host = url.host_str().unwrap_or("placeholder").to_string();
                        let mut timeouts = self.timeouts.borrow_mut();

                        if let Some(timeout) = timeouts.get_mut(&host) {
                            *timeout += self.default_timeout.1;
                            trace!("Increased waiting time for '{host}' to {}s", timeout.as_secs_f32());
                        } else {
                            timeouts.insert(host.to_string(), self.default_timeout.0);
                            trace!("Set waiting time for '{host}' {}s", self.default_timeout.0.as_secs_f32());
                        }
                    }

                    continue;
                },
                status_code => bail!("Unexpected status code '{status_code}'\n{url}"),
            };

            self.cache.borrow_mut().insert(url.clone(), content.clone());

            return Ok(content);
        }
        
        bail!("Gave up on downloading '{url}' after {} attempts", self.max_attempts);
    }

    /// Download `url` and return the content as a string.
    pub fn get_text(&'_ self, url: impl IntoUrl) -> anyhow::Result<Option<String>> {
        let content = self.get_content(url)?;
        let text = match content {
            DbSourceContent::None => None,
            DbSourceContent::Text(text) => Some(text),
            DbSourceContent::Bytes(bytes) => Some(String::from_utf8(bytes)?),
        };
        Ok(text)
    }

    /// Download `url` and parse the JSON content.
    pub fn get_json<T: DeserializeOwned>(&self, url: impl IntoUrl + Display) -> anyhow::Result<T> {
        trace!("Sending request for JSON: {url}");
        let response = self.client.get(url).send()?;
        let value: T = response.json()?;
        Ok(value)
    }

    /// Download `url` with `body` and return the JSON content.
    pub fn post_json<T: DeserializeOwned>(&self, url: impl IntoUrl + Display, body: JsonValue) -> anyhow::Result<T> {
        trace!("Sending request for JSON with body: {url}, body = {body}");
        let response = self.client.post(url).json(&body).send()?;
        let value: T = response.json()?;
        Ok(value)
    }
}

/// Weather to retry finished and failed operations.
#[derive(Debug, Default)]
pub struct RetryPolicy {
    /// Download source lists for languages that already exist.
    pub download_finished_sources: bool,

    /// Download source lists for languages that failed before.
    pub download_failed_sources: bool,

    /// Download source contents that already exist.
    pub download_finished_source: bool,

    /// Download source contents that failed before.
    pub download_failed_source: bool,

    /// Parse source contents that are already parsed.
    pub parse_finished_source: bool,

    /// Parse source contents that failed before.
    pub parse_failed_source: bool,
}

/// A translation provider that downloads and parses translations.
#[allow(clippy::type_complexity)]
pub struct Provider<'a> {
    code: String,
    name: String,
    group_name: Option<String>,
    get_sources: Box<dyn Fn(&[LangId], &DbProvider, &Downloader) -> anyhow::Result<()> + 'a>,
    parse_source: Box<dyn Fn(&DbSource) -> anyhow::Result<()> + 'a>,
}

/// How far a provider download has come.
#[derive(PartialEq, Eq)]
pub enum Progress {
    /// Sent before downloading list of sources for `lang_ids` languages.\
    /// May not be sent if `lang_ids` is empty.
    DownloadingSources { lang_ids: Vec<LanguageIdentifier> },
    /// Sent before downloading `total` sources.\
    /// May not be sent if `total` is zero.
    StartDownloadingSources { total: usize },
    /// Sent before downloading source `current` of `total`.
    DownloadingSource { current: usize, total: usize },
    /// Sent when downloading a source fails due to `error_msg`.
    FailedAtDownloadingSource { error_msg: String },
    /// Sent before parsing `total` sources.\
    /// May not be sent if `total` is zero.
    StartParsingSources { total: usize },
    /// Sent before parsing source `current` of `total`.
    ParsingSource { current: usize, total: usize },
    /// Sent when parsing a source fails due to `error_msg`.
    FailedAtParsingSource { error_msg: String },
    /// Sent after downloading `downloaded` sources and parsing `parsed` sources.\
    /// Will always be sent.
    Done { downloaded_sources: usize, parsed_sources: usize },
}

impl<'a> Provider<'a> {
    /// Create a new provider that finds sources with `get_sources`
    /// and parses them to translations with `parse_source`.
    fn new(
        code: &str, name: &str, group_name: Option<&str>,
        parse_source: impl Fn(&DbSource) -> anyhow::Result<()> + 'a,
        get_sources: impl Fn(&[LangId], &DbProvider, &Downloader) -> anyhow::Result<()> + 'a,
    ) -> Self {
        Self {
            code: code.to_string(), name: name.to_string(), group_name: group_name.map(|n| n.to_string()),
            get_sources: Box::new(get_sources),
            parse_source: Box::new(parse_source),
        }
    }

    /// Get the code name of this provider.
    pub fn code(&self) -> &str { &self.code }

    /// Get the display name of this provider.
    pub fn name(&self) -> &str { &self.name }

    /// Get the display group name of this provider.
    pub fn group_name(&self) -> Option<&str> { self.group_name.as_deref() }

    /// Download and parse translations for `lang_ids`, and save them to `provider`.
    pub fn download(
        &self,
        lang_ids: &HashSet<LanguageIdentifier>,
        provider: &DbProvider<'_>,
        downloader: &Downloader,
        retry_policy: &RetryPolicy,
        mut on_progress: impl FnMut(Progress) -> anyhow::Result<()>
    ) -> anyhow::Result<()> {
        if !retry_policy.download_failed_sources && provider.get_sources_error_message()?.is_some() {
            return Ok(());
        }

        let lang_ids: Vec<_> = if retry_policy.download_finished_sources {
            lang_ids.iter().map(|lang_id| LangId(lang_id.clone())).collect()
        } else {
            let source_lang_ids = provider.get_source_languages()?;
            lang_ids.difference(&source_lang_ids).map(|lang_id| LangId(lang_id.clone())).collect()
        };

        // Find sources
        trace!("Finding sources...");
        debug!("Languages: {}", lang_ids.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", "));
        if !lang_ids.is_empty() {
            let language_ids = lang_ids.iter()
                .map(|l| l.deref())
                .cloned()
                .collect();
            on_progress(Progress::DownloadingSources { lang_ids: language_ids })?;

            if let Err(e) = (self.get_sources)(&lang_ids, provider, downloader) {
                let error_msg = format!("Could not find sources: {e}");
                provider.set_sources_error_message(&error_msg)?;
                bail!("{error_msg}");
            };

            // Add placeholder sources for languages that were not found
            let found_languages = provider.get_source_languages()?;
            for lang_id in lang_ids {
                if found_languages.contains(&lang_id) { continue; }

                let source = provider.set_source(&lang_id, DbSourceUrls {
                    originals: None,
                    translations: Url::parse("placeholder:").expect("Invalid constant URL"),
                })?;
                source.set_contents(DbSourceContents {
                    originals: DbSourceContent::None,
                    translations: DbSourceContent::None,
                })?;
            }
        }

        // Download source contents
        trace!("Downloading source contents...");

        let mut sources_to_download = Vec::new();
        for source in provider.get_sources()? {
            if !retry_policy.download_finished_source && source.get_download_time()?.is_some() { continue; }
            if !retry_policy.download_failed_source && source.has_failed()?.is_download() { continue; }

            let urls = source.get_urls()?;
            if urls.translations.scheme() != "https" && urls.translations.scheme() != "http" { continue; }

            sources_to_download.push((source, urls));
        }

        let download_count = sources_to_download.len();
        if !sources_to_download.is_empty() { on_progress(Progress::StartDownloadingSources { total: download_count })?; }
        for (i, (source, urls)) in sources_to_download.into_iter().enumerate() {
            on_progress(Progress::DownloadingSource { current: i + 1, total: download_count })?;

            let translations_content = match downloader.get_content(urls.translations.clone()) {
                Ok(content) => content,
                Err(e) => {
                    let error_msg = format!(
                        "Could not download source translations content from '{}': {e}",
                        urls.translations,
                    );
                    source.set_error_message(&error_msg)?;
                    on_progress(Progress::FailedAtDownloadingSource { error_msg })?;
                    continue;
                }
            };
            if let Some(originals_url) = urls.originals {
                let originals_content = match downloader.get_content(originals_url.clone()) {
                    Ok(content) => content,
                    Err(e) => {
                        let error_msg = format!(
                            "Could not download source originals content from '{}': {e}",
                            originals_url,
                        );
                        source.set_error_message(&error_msg)?;
                        on_progress(Progress::FailedAtDownloadingSource { error_msg })?;
                        continue;
                    }
                };
                source.set_contents(DbSourceContents {
                    originals: originals_content,
                    translations: translations_content,
                })?;
            } else {
                source.set_contents(DbSourceContents {
                    originals: DbSourceContent::None,
                    translations: translations_content,
                })?;
            }
        }

        // Parse source contents
        trace!("Parsing source contents...");

        let mut sources_to_parse = Vec::new();
        for source in provider.get_sources()? {
            if source.get_contents()?.translations.is_none() { continue; }
            if !retry_policy.parse_finished_source && source.has_parsed()? { continue; }
            if !retry_policy.parse_failed_source && source.has_failed()?.is_some() { continue; }

            sources_to_parse.push(source);
        }

        let parse_count = sources_to_parse.len();
        if !sources_to_parse.is_empty() { on_progress(Progress::StartParsingSources { total: parse_count })?; }
        for (i, source) in sources_to_parse.into_iter().enumerate() {
            on_progress(Progress::ParsingSource { current: i + 1, total: parse_count })?;

            if let Err(e) = (self.parse_source)(&source) {
                let error_msg = format!("Could not parse source: {e}");
                source.set_error_message(&error_msg)?;
                on_progress(Progress::FailedAtParsingSource { error_msg })?;
            };
        }

        on_progress(Progress::Done { downloaded_sources: download_count, parsed_sources: parse_count })?;

        Ok(())
    }
}

/// Map of translation keys and strings for one language, with an optional comment.
type TranslationMessages = HashMap<String, (String, Option<String>)>;

struct SourceUrls {
    originals: String,
    translations: String,
}

impl<'a> Provider<'a> {
    /// Returns a closure that parses a mono source using `parse_text`.
    fn mono_text_parser(parse_text: impl Fn(String) -> anyhow::Result<Vec<DbTranslation>>) -> impl Fn(&DbSource) -> anyhow::Result<()> {
        move |source: &DbSource| {
            let DbSourceContent::Text(text) = source.get_contents()?.translations else {
                bail!("No translation text");
            };
            let translations = parse_text(text)?;
            source.set_translations(&translations)?;
            Ok(())
        }
    }

    /// Returns a closure that parses a duo source using `parse_text`.
    fn duo_text_parser(parse_text: impl Fn(String) -> anyhow::Result<TranslationMessages>) -> impl Fn(&DbSource) -> anyhow::Result<()> {
        move |source| {
            let DbSourceContents {
                originals: DbSourceContent::Text(originals),
                translations: DbSourceContent::Text(translations),
            } = source.get_contents()? else { bail!("No texts"); };

            let default_messages = parse_text(originals)?;
            let messages = parse_text(translations)?;
            let translations = merge_messages(messages, default_messages);
            source.set_translations(&translations)?;
            Ok(())
        }
    }

    fn new_mono_one_per_lang(
        code: &str, name: &str, group_name: Option<&str>,
        parse_text: impl Fn(String) -> anyhow::Result<Vec<DbTranslation>> + 'a,
        get_source_url: impl Fn(&LangId) -> String + 'a,
    ) -> Self {
        let get_sources = move |lang_ids: &[LangId], provider: &DbProvider, _: &Downloader| {
            for lang_id in lang_ids {
                let url = get_source_url(lang_id);
                let url = Url::parse(&url)?;
                provider.set_sources(&lang_id.0, &[DbSourceUrls { originals: None, translations: url }])?;
            }
            Ok(())
        };
        Self::new(code, name, group_name, Self::mono_text_parser(parse_text), get_sources)
    }

    fn new_mono_many_per_lang(
        code: &str, name: &str, group_name: Option<&str>,
        parse_text: impl Fn(String) -> anyhow::Result<Vec<DbTranslation>> + 'a,
        get_sources: impl Fn(&LangId, &Downloader) -> anyhow::Result<Vec<String>> + 'a,
    ) -> Self {
        let get_sources = move |lang_ids: &[LangId], provider: &DbProvider, downloader: &Downloader| {
            for lang_id in lang_ids {
                let urls = get_sources(lang_id, downloader)?;
                let urls: Vec<_> = urls.into_iter()
                    .map(|url| {
                        let translations = Url::parse(&url)?;
                        Ok(DbSourceUrls { originals: None, translations })
                    })
                    .collect::<anyhow::Result<_>>()?;
                provider.set_sources(lang_id, &urls)?;
            }
            Ok(())
        };
        Self::new(code, name, group_name, Self::mono_text_parser(parse_text), get_sources)
    }

    fn new_duo_one_per_lang(
        code: &str, name: &str, group_name: Option<&str>,
        parse_text: impl Fn(String) -> anyhow::Result<TranslationMessages> + 'a,
        default_source_url: &'static str,
        get_source_url: impl Fn(&LangId) -> String + 'a,
    ) -> Self {
        let get_sources = move |lang_ids: &[LangId], provider: &DbProvider, _: &Downloader| {
            let default_source_url = Url::parse(default_source_url)?;
            for lang_id in lang_ids {
                let url = get_source_url(lang_id);
                let url = Url::parse(&url)?;
                provider.set_sources(lang_id, &[DbSourceUrls {
                    originals: Some(default_source_url.clone()),
                    translations: url,
                }])?;
            }
            Ok(())
        };
        Self::new(code, name, group_name, Self::duo_text_parser(parse_text), get_sources)
    }

    fn new_duo_many_per_langs(
        code: &str, name: &str, group_name: Option<&str>,
        parse_text: impl Fn(String) -> anyhow::Result<TranslationMessages> + 'a,
        get_sources: impl for<'b> Fn(&'b [LangId], &Downloader) -> anyhow::Result<HashMap<&'b LangId, Vec<SourceUrls>>> + 'a,
    ) -> Self {
        let get_sources = move |lang_ids: &[LangId], provider: &DbProvider, downloader: &Downloader| {
            let urls = get_sources(lang_ids, downloader)?;
            for (lang_id, urls) in urls {
                let urls: Vec<_> = urls.into_iter()
                    .map(|SourceUrls { originals, translations }| {
                        let originals = Url::parse(&originals)?;
                        let translations = Url::parse(&translations)?;
                        Ok(DbSourceUrls { originals: Some(originals), translations })
                    })
                    .collect::<anyhow::Result<_>>()?;
                provider.set_sources(lang_id, &urls)?;
            }
            Ok(())
        };
        Self::new(code, name, group_name, Self::duo_text_parser(parse_text), get_sources)
    }
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

/// Creates `Translation`s by merging `messages` and `default_messages`.
pub fn merge_messages(messages: TranslationMessages, mut default_messages: TranslationMessages) -> Vec<DbTranslation> {
    let mut translations = Vec::with_capacity(messages.len());

    for (key, (translation, comment)) in messages {
        let Some((original, default_comment)) = default_messages.remove(&key) else {
            continue;
        };
        translations.push(DbTranslation {
            key: Some(key),
            original,
            translation,
            comment: comment.or(default_comment),
        });
    }

    translations
}
