mod providers;

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    fs::File,
    io::{BufReader, BufWriter},
    path::Path,
    sync::Arc,
};

use anyhow::{anyhow, bail};
use log::{debug, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use unic_langid::LanguageIdentifier;

use self::providers::{default_providers, TranslationProvider};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Translation {
    pub original: String,
    pub translation: String,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct TranslationStore {
    #[serde(skip, default = "default_providers")]
    providers: Vec<Arc<dyn TranslationProvider + Send + Sync>>,

    pub translations: HashMap<String, HashMap<LanguageIdentifier, Option<Vec<Translation>>>>,
}

impl Default for TranslationStore {
    fn default() -> Self {
        Self {
            providers: default_providers(),
            translations: HashMap::new(),
        }
    }
}

impl TranslationStore {
    pub fn from_file(file: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let file = File::open(&file)
            .map_err(|e| anyhow!("Could not open file {:?}: {e}", file.as_ref()))?;
        let reader = BufReader::new(file);
        #[cfg(debug_assertions)]
        let mut store: TranslationStore = serde_json::from_reader(reader)?;
        #[cfg(not(debug_assertions))]
        let mut store: TranslationStore = bincode::deserialize_from(reader)?;

        store.translations.retain(|scope, _| {
            store
                .providers
                .iter()
                .any(|provider| provider.id() == scope)
        });

        Ok(store)
    }

    pub fn write_to(&self, file: impl AsRef<Path>) -> Result<(), anyhow::Error> {
        let file = File::create(&file)
            .map_err(|e| anyhow!("Could not create file {:?}: {e}", file.as_ref()))?;
        let writer = BufWriter::new(file);
        #[cfg(debug_assertions)]
        serde_json::to_writer_pretty(writer, self)?;
        #[cfg(not(debug_assertions))]
        bincode::serialize_into(writer, self)?;
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
