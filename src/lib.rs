// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod config;
mod providers;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    iter,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::bail;
use async_trait::async_trait;
use log::{debug, error};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::{task::JoinSet, time::timeout};
use unic_langid::LanguageIdentifier;

use self::providers::{default_providers, merge_messages, simple_provider, SimpleProvider};

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
    pub finished: bool,
    pub translation_bundles: BTreeMap<String, TranslationBundle>,
}

pub type TranslationBundle = BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Translation {
    pub original: String,
    pub translation: String,
    pub comment: Option<String>,
    pub key: Option<String>,
    pub source: String,
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

impl TranslationStore {
    /// Returns a new `TranslationStore` that loads and saves translations to `save_path`.
    pub fn new(save_path: PathBuf) -> Self {
        Self {
            providers: default_providers(),
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
