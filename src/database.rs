// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, HashMap, HashSet, hash_map::Entry},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use log::{debug, error, trace, warn};
use reqwest::Client;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use tokio::{task::JoinSet, time::timeout};
use unic_langid::LanguageIdentifier;

use crate::providers::{
    builtin::builtin_providers,
    ProviderCache, ProviderCacheMultiple, SimpleProvider,
    Translation, TranslationBundle, TranslationProvider,
    merge_messages, simple_provider,
};

/// SQL to initialize the SQLite database.
const INIT_SQL: &str = "
CREATE TABLE Languages (
    id INTEGER PRIMARY KEY,
    code TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE Providers (
    id INTEGER PRIMARY KEY,
    from_file  INTEGER NOT NULL DEFAULT 0,
    code       TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    group_name TEXT,
    downloaded INTEGER NOT NULL DEFAULT 0,
    failed     INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE Sources (
    id INTEGER PRIMARY KEY,

    provider_id INTEGER NOT NULL,
    language_id INTEGER NOT NULL,

    originals_url     TEXT UNIQUE,
    translations_url  TEXT NOT NULL UNIQUE,

    downloaded        INTEGER,
    originals_text    TEXT,
    translations_text TEXT,

    failed            INTEGER NOT NULL DEFAULT 0,

    FOREIGN KEY (provider_id) REFERENCES Providers(id),
    FOREIGN KEY (language_id) REFERENCES Languages(id)
) STRICT;

CREATE TABLE Translations (
    id INTEGER PRIMARY KEY,

    source_id   INTEGER NOT NULL,

    key         TEXT,
    original    TEXT NOT NULL,
    translation TEXT NOT NULL,
    comment     TEXT,

    FOREIGN KEY (source_id)   REFERENCES Sources(id)
) STRICT;

CREATE INDEX Translations_SourceId ON Translations (source_id);
CREATE INDEX Sources_ProviderId ON Sources (provider_id);
";

pub struct TranslationStore {
    providers: Vec<Arc<dyn TranslationProvider + Send + Sync>>,
    save_path: PathBuf,
    pub provider_caches: BTreeMap<String, ProviderCache>,
}

impl TranslationStore {
    /// Returns a new `TranslationStore` that loads and saves translations to `save_path`.
    pub fn new(save_path: PathBuf) -> Self {
        Self {
            providers: builtin_providers(),
            save_path,
            provider_caches: BTreeMap::new(),
        }
    }

    pub fn provider(&self, id: &str) -> Option<&Arc<dyn TranslationProvider + Send + Sync>> {
        self.providers.iter().find(|provider| provider.id() == id)
    }

    pub fn providers(&self) -> impl Iterator<Item = &Arc<dyn TranslationProvider + Send + Sync>> {
        self.providers.iter()
    }

    pub fn languages(&self) -> HashSet<&LanguageIdentifier> {
        self.provider_caches
            .values()
            .flat_map(|provider_cache| provider_cache.translation_bundles())
            .flat_map(|bundle| bundle.keys())
            .collect()
    }

    /// Returns an iterator over all the translations, together with the provider and language identifiers.
    pub fn translations(
        &self,
    ) -> impl Iterator<Item = (&String, &LanguageIdentifier, &Translation)> {
        self.provider_caches
            .iter()
            .flat_map(|(id, provider_cache)| {
                provider_cache
                    .translation_bundles()
                    .map(move |bundle| (id, bundle))
            })
            .flat_map(|(id, bundle)| {
                bundle
                    .iter()
                    .map(move |(lang_id, translations)| (id, lang_id, translations))
            })
            .filter_map(|(id, lang_id, translations)| {
                translations
                    .as_ref()
                    .map(|translations| (id, lang_id, translations))
            })
            .flat_map(|(id, lang_id, translations)| {
                translations
                    .iter()
                    .map(move |translation| (id, lang_id, translation))
            })
    }

    pub async fn generate(
        &mut self,
        lang_ids: Vec<LanguageIdentifier>,
        provider_ids: Vec<String>,
        remove_failed: bool,
        client: Client,
    ) -> Result<HashMap<String, Option<String>>, anyhow::Error> {
        if lang_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut join_set = JoinSet::new();

        for provider_id in provider_ids {
            let Some(provider) = self
                .providers
                .iter()
                .find(|provider| provider.id() == provider_id)
                .cloned()
            else {
                bail!("Provider not found: {provider_id}");
            };
            if provider.temporary() {
                debug!("Skipping generating temporary provider: {}", provider.id());
                continue;
            }

            let unfinished = self
                .provider_caches
                .get(&provider_id)
                .is_some_and(|provider_cache| match provider_cache {
                    ProviderCache::Single(_) => false,
                    ProviderCache::Multiple(multiple) => !multiple.finished,
                });
            let previous = if unfinished {
                let previous = self.provider_caches.remove(&provider_id);
                match previous {
                    Some(ProviderCache::Multiple(multiple)) => Some(multiple),
                    _ => None,
                }
            } else {
                None
            };
            let client = client.clone();
            let lang_ids = lang_ids.clone();
            join_set.spawn(timeout(Duration::from_secs(60), async move {
                (
                    provider_id,
                    provider.generate(previous, lang_ids, client).await,
                )
            }));
        }

        let mut errors = HashMap::new();

        while let Some(join) = join_set.join_next().await {
            let (provider_id, provider_cache) = match join {
                Ok(Ok(t)) => t,
                Ok(Err(e)) => {
                    error!("Could not generate: {e}");
                    continue;
                }
                Err(e) => {
                    error!("Could not generate: {e}");
                    continue;
                }
            };
            let provider_cache = match provider_cache {
                Ok(provider_cache) => provider_cache,
                Err(e) => {
                    error!("Could not generate '{provider_id}': {e}");
                    if remove_failed {
                        self.provider_caches.remove(&provider_id);
                    }
                    errors.insert(provider_id, Some(e.to_string()));
                    continue;
                }
            };

            debug!(
                "Generated '{provider_id}': up to possibly {} translations per language",
                provider_cache
                    .translation_bundles()
                    .map(|bundle| {
                        bundle
                            .values()
                            .filter_map(|translations| translations.as_ref())
                            .map(|translations| translations.len())
                            .max()
                            .unwrap_or(0)
                    })
                    .sum::<usize>()
            );

            errors.insert(provider_id.clone(), None);
            self.provider_caches.insert(provider_id, provider_cache);
        }

        Ok(errors)
    }
}

impl TranslationStore {
    /// Load translations from the save path.
    ///
    /// Returns `false` if no file were found at the save path.
    pub fn load_translations(&mut self) -> anyhow::Result<bool> {
        if !self.save_path.exists() || !self.save_path.is_file() {
            return Ok(false);
        }

        let now = Instant::now();

        let conn = Connection::open(&self.save_path)
            .map_err(|e| anyhow!("Could not open database file {:?}: {e}", self.save_path))?;

        let mut provider_caches: HashMap<String, ProviderCache> = HashMap::new();

        let mut languages_stmt = conn.prepare("SELECT id, code FROM Languages")?;
        let languages: Vec<_> = languages_stmt
            .query_map((), |row| {
                let id: i64 = row.get(0)?;
                let code = LanguageIdentifier::from_str(row.get_ref(1)?.as_str()?).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
                })?;
                Ok((id, code))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut providers_stmt = conn.prepare("SELECT id, code FROM Providers")?;
        let providers: Vec<_> = providers_stmt
            .query_map((), |row| {
                let id: i64 = row.get(0)?;
                let code: String = row.get(1)?;
                Ok((id, code))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut translations_stmt = conn.prepare("
            SELECT Sources.translations_url as source, key, original, translation, comment FROM Translations
            JOIN Sources ON Translations.source_id = Sources.id
            WHERE Sources.provider_id = ? AND Sources.language_id = ?
        ")?;

        for (provider_id, provider_code) in providers {
            let mut bundle: TranslationBundle = BTreeMap::new();

            for (language_id, language_code) in &languages {
                let translations: Vec<Translation> = translations_stmt
                    .query_map((provider_id, language_id), |row| Ok(Translation {
                        source: row.get(0)?,
                        key: row.get(1)?,
                        original: row.get(2)?,
                        translation: row.get(3)?,
                        comment: row.get(4)?,
                    }))?
                    .collect::<rusqlite::Result<_>>()?;

                let translations = if translations.is_empty() { None } else { Some(translations) };
                bundle.insert(language_code.clone(), translations);
            }

            provider_caches.insert(provider_code.clone(), ProviderCache::Single(bundle));

            trace!("Read '{provider_code}' in {} seconds", now.elapsed().as_secs());
        }

        debug!("Read cache file in {} seconds", now.elapsed().as_secs());

        provider_caches.retain(|scope, _| {
            let retain = self.providers.iter().any(|provider| provider.id() == scope);
            if !retain {
                warn!("Unknown provider in translation cache: {scope}");
            }
            retain
        });

        self.provider_caches.extend(provider_caches);

        Ok(true)
    }

    /// Write translations to the save path.
    pub fn save_translations(&self) -> anyhow::Result<()> {
        let now = Instant::now();

        let mut temp_save_path = self.save_path.clone();
        let Some(file_name) = self.save_path.file_name() else {
            bail!("Save path has no file name: {:?}", self.save_path);
        };
        temp_save_path.set_file_name(format!("~{}", file_name.to_string_lossy()));

        if temp_save_path.exists() {
            fs::remove_file(&temp_save_path).map_err(|e| {
                anyhow!("Could not delete former temporary database file '{temp_save_path:?}': {e}")
            })?;
        }
        let conn = Connection::open(&temp_save_path).map_err(|e| {
            anyhow!("Could not create temporary database file '{temp_save_path:?}': {e}")
        })?;
        conn.execute_batch(INIT_SQL)?;

        let provider_caches = self
            .provider_caches
            .iter()
            .filter(|(provider_id, _)| {
                self.provider(provider_id)
                    .is_some_and(|provider| !provider.temporary())
            });

        let mut language_indices: HashMap<&LanguageIdentifier, i64> = HashMap::new();
        let mut provider_indices: HashMap<&String, i64> = HashMap::new();
        let mut source_indices: HashMap<&String, i64> = HashMap::new();

        conn.execute("BEGIN", ())?;
        for (provider_id, provider_cache) in provider_caches {
            if let Entry::Vacant(e) = provider_indices.entry(provider_id) {
                conn.execute("INSERT INTO Providers (code) VALUES (?)", [provider_id])?;
                let rowid = conn.query_one("SELECT last_insert_rowid()", (), |r| r.get(0))?;
                e.insert(rowid);
            }

            for (language_id, translations) in provider_cache.translation_bundles().flatten() {
                let Some(translations) = translations else { continue; };

                if let Entry::Vacant(e) = language_indices.entry(language_id) {
                    conn.execute("INSERT INTO Languages (code) VALUES (?)", [language_id.to_string()])?;
                    let rowid = conn.query_one("SELECT last_insert_rowid()", (), |r| r.get(0))?;
                    e.insert(rowid);
                }

                for translation in translations {
                    if let Entry::Vacant(e) = source_indices.entry(&translation.source) {
                        conn.execute("INSERT INTO Sources (provider_id, language_id, translations_url, downloaded) VALUES (?, ?, ?, ?)", (
                            provider_indices.get(provider_id),
                            language_indices.get(language_id),
                            &translation.source,
                            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs() as u32
                        ))?;
                        let rowid = conn.query_one("SELECT last_insert_rowid()", (), |a| a.get(0))?;
                        e.insert(rowid);
                    }
                    conn.execute(
                        "INSERT INTO Translations (source_id, key, original, translation, comment) VALUES (?, ?, ?, ?, ?)",
                        params![
                            source_indices.get(&translation.source),
                            translation.key,
                            translation.original,
                            translation.translation,
                            translation.comment,
                        ],
                    )?;
                }
            }
        }
        conn.execute("COMMIT", ())?;
        conn.close().map_err(|(_, e)| e)?; // Cannot move file without closing it

        fs::rename(&temp_save_path, &self.save_path).map_err(|e| {
            anyhow!(
                "Could not move temporary database file from '{temp_save_path:?}' to '{:?}': {e}",
                self.save_path
            )
        })?;

        debug!("Wrote cache file in {} seconds", now.elapsed().as_secs());
        Ok(())
    }
}

// Config

#[derive(Deserialize, Serialize)]
struct Config {
    translations_path: Option<PathBuf>,
    translations: Vec<ConfigEntry>,
}

#[derive(Deserialize, Serialize)]
struct ConfigEntry {
    #[serde(rename = "type")]
    type_name: String,
    name: String,
    group_name: Option<String>,
    paths: Vec<ConfigEntryPath>,
}

#[derive(Deserialize, Serialize)]
struct ConfigEntryPath {
    language: LanguageIdentifier,
    path: String,
}

/** Provider wich does not provide anything. */
struct DummyProvider {
    id: String,
    name: String,
    group_name: Option<String>,
}

#[async_trait]
impl TranslationProvider for DummyProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn group_name(&self) -> Option<&str> {
        self.group_name.as_deref()
    }

    fn temporary(&self) -> bool {
        true
    }

    async fn generate(
        &self,
        _previous: Option<ProviderCacheMultiple>,
        _lang_ids: Vec<LanguageIdentifier>,
        _client: Client,
    ) -> anyhow::Result<ProviderCache> {
        bail!("Should already be generated: {}", &self.id)
    }
}

impl TranslationStore {
    /// Read and load a TOML config file from `file`.
    pub fn load_config(&mut self, file: impl AsRef<Path>) -> anyhow::Result<()> {
        let now = Instant::now();

        let mut file = File::open(file)?;
        let mut toml = String::with_capacity(
            file.metadata()
                .map_or(0, |metadata| metadata.len() as usize),
        );
        file.read_to_string(&mut toml)?;
        let config: Config = toml::from_str(&toml)?;

        if let Some(translations_path) = config.translations_path {
            self.save_path = translations_path;
        }

        let mut config_texts = Vec::with_capacity(config.translations.len());
        for entry in &config.translations {
            let mut texts = HashMap::with_capacity(entry.paths.len());
            for entry in &entry.paths {
                let mut file = File::open(&entry.path)
                    .map_err(|e| anyhow!("Could not open file ({}): {e}", entry.path))?;
                let mut text = String::with_capacity(
                    file.metadata()
                        .map_or(0, |metadata| metadata.len() as usize),
                );
                file.read_to_string(&mut text)
                    .map_err(|e| anyhow!("Could not read file ({}): {e}", entry.path))?;
                texts.insert(&entry.language, (text, &entry.path));
            }
            config_texts.push((entry, texts));
        }

        for (i, (entry, texts)) in config_texts.into_iter().enumerate() {
            let translation_bundle = match simple_provider(&entry.type_name) {
                Some(SimpleProvider::Duo(parse)) => {
                    let mut translation_bundle = BTreeMap::new();

                    let en: LanguageIdentifier = "en".parse().unwrap();

                    let text = texts
                        .get(&en)
                        .map(|(text, _)| text)
                        .ok_or_else(|| anyhow!("Entry '{}' has no language 'en'", entry.name))?
                        .to_string();
                    let messages_en = parse(text).map_err(|e| {
                        anyhow!(
                            "Could not parse language 'en' in entry '{}': {e}",
                            entry.name
                        )
                    })?;

                    for (lang_id, (text, path)) in texts {
                        if *lang_id == en {
                            continue;
                        }
                        let messages = parse(text).map_err(|e| {
                            anyhow!(
                                "Could not parse language '{lang_id}' in entry '{}': {e}",
                                entry.name
                            )
                        })?;

                        translation_bundle.insert(
                            lang_id.clone(),
                            Some(merge_messages(messages, &messages_en, path)),
                        );
                    }

                    translation_bundle
                }
                Some(SimpleProvider::Mono(parse)) => {
                    let mut translation_bundle = BTreeMap::new();

                    for (lang_id, (text, path)) in texts {
                        let translations = parse(text, path).map_err(|e| {
                            anyhow!(
                                "Could not parse language '{lang_id}' in entry '{}': {e}",
                                entry.name
                            )
                        })?;
                        translation_bundle.insert(lang_id.clone(), Some(translations));
                    }

                    translation_bundle
                }
                None => bail!("Type '{}' is not supported", entry.type_name),
            };

            let mut id = format!("local:{i}-");
            for c in entry.name.chars() {
                if c.is_ascii_alphanumeric() {
                    id.push(c.to_ascii_lowercase());
                } else if c == ' ' {
                    id.push('-');
                }
            }
            let id = id.trim_end_matches('-');

            trace!(
                "Read config entry '{}' (ID {id}): {} translations",
                entry.name,
                translation_bundle
                    .values()
                    .filter_map(|translations| translations.as_ref())
                    .flatten()
                    .count(),
            );
            self.provider_caches
                .insert(id.to_string(), ProviderCache::Single(translation_bundle));
            self.providers.push(Arc::new(DummyProvider {
                id: id.to_string(),
                name: entry.name.clone(),
                group_name: entry.group_name.clone(),
            }));
        }

        debug!("Read config in {} seconds", now.elapsed().as_secs());
        Ok(())
    }
}
