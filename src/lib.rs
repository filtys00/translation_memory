// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod providers;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    fs::File,
    io::{BufReader, BufWriter, Read},
    path::Path,
    sync::Arc,
    time::Instant,
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use log::{debug, error, trace};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use unic_langid::LanguageIdentifier;
use xz2::{read::XzDecoder, write::XzEncoder};

use self::providers::{default_providers, TranslationProvider};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Translation {
    pub original: String,
    pub translation: String,
    pub comment: Option<String>,
}

pub struct TranslationStore {
    providers: Vec<Arc<dyn TranslationProvider + Send + Sync>>,

    pub translations: BTreeMap<String, BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>>,
}

#[derive(Deserialize, Serialize)]
struct Config {
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

struct NothingProvider {
    id: String,
    name: String,
    group_name: Option<String>,
}

#[async_trait]
impl TranslationProvider for NothingProvider {
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

impl Default for TranslationStore {
    fn default() -> Self {
        Self {
            providers: default_providers(),
            translations: BTreeMap::new(),
        }
    }
}

impl TranslationStore {
    pub fn load_translations(&mut self, file: impl AsRef<Path>) -> anyhow::Result<()> {
        let now = Instant::now();
        let file = File::open(&file)
            .map_err(|e| anyhow!("Could not open file {:?}: {e}", file.as_ref()))?;
        let reader = BufReader::new(file);
        let reader = XzDecoder::new(reader);
        self.translations = bincode::deserialize_from(reader)?;
        debug!("Read cache file in {} seconds", now.elapsed().as_secs());

        self.translations.retain(|scope, _| {
            let retain = self.providers.iter().any(|provider| provider.id() == scope);
            if !retain {
                trace!("Unknown provider in translation cache: {scope}");
            }
            retain
        });

        Ok(())
    }

    pub fn load_config(&mut self, file: impl AsRef<Path>) -> anyhow::Result<()> {
        let now = Instant::now();

        let mut file = File::open(file)?;
        let mut toml = String::with_capacity(
            file.metadata()
                .map_or(0, |metadata| metadata.len() as usize),
        );
        file.read_to_string(&mut toml)?;
        let config: Config = toml::from_str(&toml)?;

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
                file_type => bail!("Unsupported translations type: {file_type}"),
            };

            let id = format!("localfile:{i}");
            self.translations.entry(id.clone()).or_insert(translations);
            self.providers.push(Arc::new(NothingProvider {
                id,
                name: entry.name.clone(),
                group_name: entry.group_name.clone(),
            }));
        }

        debug!("Read config in {} seconds", now.elapsed().as_secs());
        Ok(())
    }

    pub fn write_to_file(&self, file: impl AsRef<Path>) -> anyhow::Result<()> {
        let now = Instant::now();

        let file = File::create(&file)
            .map_err(|e| anyhow!("Could not create file {:?}: {e}", file.as_ref()))?;
        let writer = BufWriter::new(file);
        let writer = XzEncoder::new(writer, 0);

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

            let client = client.clone();
            let lang_ids = lang_ids.clone();

            join_set.spawn(async move { (id, provider.generate(lang_ids, client).await) });
        }

        let mut errors = HashMap::new();

        while let Some(join) = join_set.join_next().await {
            let (id, result) = match join {
                Ok(t) => t,
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
