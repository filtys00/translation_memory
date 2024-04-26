// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod providers;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    fs::{self, File},
    io::{BufReader, BufWriter, Read},
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

use self::providers::{default_providers, parse_android, parse_microsoft_tbx, TranslationProvider};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Translation {
    pub original: String,
    pub translation: String,
    pub comment: Option<String>,
}

pub struct TranslationStore {
    providers: Vec<Arc<dyn TranslationProvider + Send + Sync>>,

    save_path: PathBuf,

    pub translations: BTreeMap<String, BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>>,
}

#[derive(Deserialize, Serialize)]
struct Config {
    translations_path: Option<PathBuf>,
    translations: Vec<ConfigEntry>,
}

#[derive(Deserialize, Serialize)]
struct ConfigEntry {
    #[serde(rename = "type")]
    format: String,
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
        _lang_ids: Vec<LanguageIdentifier>,
        _client: Arc<Client>,
    ) -> anyhow::Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>> {
        bail!("Should already be generated: {}", &self.id)
    }
}

impl TranslationStore {
    /// Returns a new `TranslationStore` that loads and saves translations to `save_path`.
    pub fn new(save_path: PathBuf) -> Self {
        Self {
            providers: default_providers(),
            save_path,
            translations: BTreeMap::new(),
        }
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
        let reader = BufReader::new(file);
        let reader = GzDecoder::new(reader);
        let mut translations: HashMap<
            String,
            BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>,
        > = bincode::deserialize_from(reader)?;
        debug!("Read cache file in {} seconds", now.elapsed().as_secs());

        translations.retain(|scope, _| {
            let retain = self.providers.iter().any(|provider| provider.id() == scope);
            if !retain {
                warn!("Unknown provider in translation cache: {scope}");
            }
            retain
        });

        self.translations.extend(translations);

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
                texts.insert(entry.language.clone(), text);
            }
            config_texts.push((entry, texts));
        }

        for (i, (entry, texts)) in config_texts.into_iter().enumerate() {
            let translations = match entry.format.as_str() {
                "androidxml" => {
                    let mut translations = BTreeMap::new();

                    let en: LanguageIdentifier = "en".parse().unwrap();

                    let text = texts
                        .get(&en)
                        .ok_or_else(|| anyhow!("Entry '{}' has no language 'en'", entry.name))?
                        .to_string();
                    let messages_en = parse_android(text)?;

                    for (lang_id, text) in texts {
                        if lang_id == en {
                            continue;
                        }
                        let messages = parse_android(text)
                            .map_err(|e| anyhow!("Could not parse type 'androidxml': {e}"))?;

                        let mut t = Vec::with_capacity(messages.len());
                        for (key, (translation, _comment)) in messages {
                            let Some((original, comment)) = messages_en.get(&key) else {
                                continue;
                            };
                            t.push(Translation {
                                original: original.clone(),
                                translation,
                                comment: comment.as_ref().cloned(),
                            });
                        }
                        translations.insert(lang_id, Some(t));
                    }

                    translations
                }
                "microsofttbx" => {
                    let mut translations = BTreeMap::new();

                    for (lang_id, text) in texts {
                        let t = parse_microsoft_tbx(text)
                            .map_err(|e| anyhow!("Could not parse type 'microsofttbx': {e}"))?;
                        translations.insert(lang_id, Some(t));
                    }

                    translations
                }
                file_type => bail!("Unsupported translations type: {file_type}"),
            };

            let id = format!("localfile:{i}");
            trace!(
                "Read config entry '{}' (ID {id}): {} translations",
                entry.name,
                translations.len()
            );
            self.translations.entry(id.clone()).or_insert(translations);
            self.providers.push(Arc::new(DummyProvider {
                id,
                name: entry.name.clone(),
                group_name: entry.group_name.clone(),
            }));
        }

        debug!("Read config in {} seconds", now.elapsed().as_secs());
        Ok(())
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
        let writer = BufWriter::new(file);
        let writer = GzEncoder::new(writer, Compression::fast());

        let mut translations: HashMap<
            &String,
            &BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>,
        > = self.translations.iter().collect();
        translations.retain(|scope, _| {
            self.providers
                .iter()
                .find(|provider| provider.id() == *scope)
                .map_or(false, |provider| !provider.temporary())
        });

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

    pub async fn generate(
        &mut self,
        lang_ids: Vec<LanguageIdentifier>,
        ids: Vec<String>,
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

        for id in ids {
            let Some(provider) = self
                .providers
                .iter()
                .find(|provider| provider.id() == id)
                .cloned()
            else {
                bail!("Provider not found: {id}");
            };
            if provider.temporary() {
                debug!("Skipping generating temporary provider: {}", provider.id());
                continue;
            }

            let client = client.clone();
            let lang_ids = lang_ids.clone();
            join_set.spawn(timeout(Duration::from_secs(60), async move {
                (id, provider.generate(lang_ids, client).await)
            }));
        }

        let mut errors = HashMap::new();

        while let Some(join) = join_set.join_next().await {
            let (id, result) = match join {
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
            let t = match result {
                Ok(t) => t,
                Err(e) => {
                    error!("Could not generate '{id}': {e}");
                    if remove_failed {
                        self.translations.remove(&id);
                    }
                    errors.insert(id, Some(e.to_string()));
                    continue;
                }
            };

            debug!(
                "Generated '{id}': up to {} translations for each language",
                t.iter()
                    .filter_map(|(_, t)| t.as_ref())
                    .map(|t| t.len())
                    .max()
                    .unwrap_or(0)
            );

            errors.insert(id.clone(), None);
            self.translations.insert(id, t);
        }

        Ok(errors)
    }

    pub fn providers(&self) -> &[Arc<dyn TranslationProvider + Send + Sync>] {
        &self.providers
    }

    pub fn provider(&self, id: &str) -> Option<&Arc<dyn TranslationProvider + Send + Sync>> {
        self.providers.iter().find(|provider| provider.id() == id)
    }

    pub fn languages(&self) -> HashSet<&LanguageIdentifier> {
        self.translations
            .iter()
            .flat_map(|(_, t)| t.iter())
            .map(|(lang_id, _)| lang_id)
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &LanguageIdentifier, &Translation)> {
        self.translations
            .iter()
            .flat_map(|(scope, t)| t.iter().map(move |t| (scope, t)))
            .filter_map(|(scope, (lang_id, t))| t.as_ref().map(|t| (scope, lang_id, t)))
            .flat_map(|(scope, lang_id, t)| t.iter().map(move |t| (scope, lang_id, t)))
    }
}
