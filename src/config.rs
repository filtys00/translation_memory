// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use log::{debug, trace, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use super::{
    merge_messages, simple_provider, ProviderCache, ProviderCacheMultiple, SimpleProvider,
    Translation, TranslationProvider, TranslationStore,
};

/// The currently used version of the cache file format.
///
/// # Version changes
/// - `0` => `1`:
///   Add `ProviderCache` together with `ProviderCache::Multiple`
const CURRENT_CACHE_FILE_VERSION: u8 = 1;

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
