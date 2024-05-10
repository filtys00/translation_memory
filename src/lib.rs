// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod providers;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    iter,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use log::{debug, error, trace, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::{task::JoinSet, time::timeout};
use unic_langid::LanguageIdentifier;

use self::providers::{
    default_providers, merge_messages, simple_provider, SimpleProvider, TranslationProvider,
};

/// The currently used version of the cache file format.
///
/// # Version changes
/// - `0` => `1`:
///   Add `ProviderCache` together with `ProviderCache::Multiple`
const CURRENT_CACHE_FILE_VERSION: u8 = 1;

pub struct TranslationStore {
    providers: Vec<Arc<dyn TranslationProvider + Send + Sync>>,
    save_path: PathBuf,
    pub provider_caches: BTreeMap<String, ProviderCache>,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ProviderCache {
    Single(TranslationBundle),
    Multiple(ProviderCacheMultiple),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProviderCacheMultiple {
    /// If all available translation bundles have been added to `translation_bundles`.
    finished: bool,
    translation_bundles: BTreeMap<String, TranslationBundle>,
}

type TranslationBundle = BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Translation {
    pub original: String,
    pub translation: String,
    pub comment: Option<String>,
    pub key: Option<String>,
    pub source: String,
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
        _client: Arc<Client>,
    ) -> anyhow::Result<ProviderCache> {
        bail!("Should already be generated: {}", &self.id)
    }
}

impl TranslationStore {
    /// Returns a new `TranslationStore` that loads and saves translations to `save_path`.
    pub fn new(save_path: PathBuf) -> Self {
        Self {
            providers: default_providers(),
            save_path,
            provider_caches: BTreeMap::new(),
        }
    }

    /// Write translations to the save path.
    pub fn save_translations(&self) -> anyhow::Result<()> {
        let now = Instant::now();

        let mut temp_save_path = self.save_path.clone();
        let Some(file_name) = self.save_path.file_name() else {
            bail!("Save path has no file name: {:?}", self.save_path);
        };
        temp_save_path.set_file_name(format!("~{}", file_name.to_string_lossy()));

        let file = File::create(&temp_save_path).map_err(|e| {
            anyhow!("Could not create temporary save file '{temp_save_path:?}': {e}")
        })?;
        let mut writer = BufWriter::new(file);
        writer.write_all(&[CURRENT_CACHE_FILE_VERSION])?;
        let writer = GzEncoder::new(writer, Compression::fast());

        let translations: HashMap<&String, &ProviderCache> = self
            .provider_caches
            .iter()
            .filter(|(provider_id, _)| {
                self.provider(provider_id)
                    .map_or(false, |provider| !provider.temporary())
            })
            .collect();

        bincode::serialize_into(writer, &translations)?;

        fs::rename(&temp_save_path, &self.save_path).map_err(|e| {
            anyhow!(
                "Could not move temporary save file from '{temp_save_path:?}' to '{:?}': {e}",
                self.save_path
            )
        })?;

        debug!("Wrote cache file in {} seconds", now.elapsed().as_secs());
        Ok(())
    }

    /// Load translations from the save path.
    ///
    /// Returns `false` if no file were found at the save path.
    pub fn load_translations(&mut self) -> anyhow::Result<bool> {
        if !self.save_path.exists() || !self.save_path.is_file() {
            return Ok(false);
        }

        let now = Instant::now();

        let file = File::open(&self.save_path)
            .map_err(|e| anyhow!("Could not open file {:?}: {e}", self.save_path))?;
        let mut reader = BufReader::new(file);

        let mut version = [0; 1];
        reader.read_exact(&mut version)?;
        let reader = GzDecoder::new(reader);
        let mut provider_caches = match version[0] {
            0 => {
                let translations: HashMap<
                    String,
                    BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>,
                > = bincode::deserialize_from(reader)?;
                translations
                    .into_iter()
                    .map(|(id, scope)| (id, ProviderCache::Single(scope)))
                    .collect()
            }
            CURRENT_CACHE_FILE_VERSION => {
                let translations: HashMap<String, ProviderCache> =
                    bincode::deserialize_from(reader)?;
                translations
            }
            version if version > CURRENT_CACHE_FILE_VERSION => {
                bail!("Cache file version '{version}' is too new");
            }
            version => {
                bail!("Cache file version '{version}' is unsupported");
            }
        };

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
                let mut file = File::open(&entry.path)?;
                let mut text = String::with_capacity(
                    file.metadata()
                        .map_or(0, |metadata| metadata.len() as usize),
                );
                file.read_to_string(&mut text)?;
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

            let id = format!("localfile:{i}");
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
                .insert(id.clone(), ProviderCache::Single(translation_bundle));
            self.providers.push(Arc::new(DummyProvider {
                id,
                name: entry.name.clone(),
                group_name: entry.group_name.clone(),
            }));
        }

        debug!("Read config in {} seconds", now.elapsed().as_secs());
        Ok(())
    }

    pub fn provider(&self, id: &str) -> Option<&Arc<dyn TranslationProvider + Send + Sync>> {
        self.providers.iter().find(|provider| provider.id() == id)
    }

    pub fn providers(&self) -> &[Arc<dyn TranslationProvider + Send + Sync>] {
        &self.providers
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
    ) -> Result<HashMap<String, Option<String>>, anyhow::Error> {
        if lang_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let client = Arc::new(
            Client::builder()
                .user_agent(concat!(
                    env!("CARGO_PKG_NAME"),
                    "/",
                    env!("CARGO_PKG_VERSION"),
                ))
                .build()?,
        );

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

            let unfinished =
                self.provider_caches
                    .get(&provider_id)
                    .map_or(false, |provider_cache| match provider_cache {
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
