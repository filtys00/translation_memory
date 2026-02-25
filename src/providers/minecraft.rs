// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
};

use anyhow::anyhow;
use async_trait::async_trait;
use log::trace;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;
use zip::ZipArchive;

use super::{ProviderCache, ProviderCacheMultiple, Translation, TranslationProvider, lang_id_to_string};

pub struct MinecraftProvider;

#[async_trait]
impl TranslationProvider for MinecraftProvider {
    fn id(&self) -> &str {
        "minecraft"
    }

    fn name(&self) -> &str {
        "Minecraft"
    }

    async fn generate(
        &self,
        _previous: Option<ProviderCacheMultiple>,
        lang_ids: Vec<LanguageIdentifier>,
        client: Client,
    ) -> anyhow::Result<ProviderCache> {
        let manifest: Manifest = client
            .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
            .send()
            .await?
            .json()
            .await?;

        let version = manifest
            .versions
            .iter()
            .find(|version| version.id == manifest.latest.release)
            .ok_or_else(|| {
                anyhow!("Malformed Minecraft manifest: 'latest.release' does not exist")
            })?;
        let version: Version = client.get(&version.url).send().await?.json().await?;

        let asset_index: AssetIndex = client
            .get(version.asset_index.url)
            .send()
            .await?
            .json()
            .await?;

        let mut translation_bundle = BTreeMap::new();

        let messages_en: HashMap<String, String> = {
            let bytes = client
                .get(&version.downloads.client.url)
                .send()
                .await?
                .bytes()
                .await?;
            let mut zip = ZipArchive::new(Cursor::new(bytes))?;
            let lang_file = zip.by_name("assets/minecraft/lang/en_us.json")?;
            serde_json::from_reader(lang_file)?
        };

        for lang_id in lang_ids {
            let key = format!(
                "minecraft/lang/{}.json",
                lang_id_to_string(&lang_id, "_", false, "_", false),
            );

            let Some(object) = asset_index.objects.get(&key) else {
                translation_bundle.insert(lang_id, None);
                continue;
            };
            let url = format!(
                "https://resources.download.minecraft.net/{}/{}",
                object.hash.get(0..2).unwrap_or(""),
                object.hash,
            );
            let messages: HashMap<String, String> = client.get(&url).send().await?.json().await?;

            let mut translations = Vec::with_capacity(messages.len());
            for (key, message) in messages {
                let Some(message_en) = messages_en.get(&key) else {
                    trace!("Translation key '{key}' were found in '{lang_id}' translation but not in default translation");
                    continue;
                };

                translations.push(Translation {
                    original: message_en.clone(),
                    translation: message,
                    comment: None,
                    key: Some(key),
                    source: url.clone(),
                });
            }
            translation_bundle.insert(lang_id, Some(translations));
        }

        Ok(ProviderCache::Single(translation_bundle))
    }
}

#[derive(Deserialize, Serialize)]
struct Manifest {
    latest: ManifestLatest,
    versions: Vec<ManifestVersion>,
}

#[derive(Deserialize, Serialize)]
struct ManifestLatest {
    release: String,
    snapshot: String,
}

#[derive(Deserialize, Serialize)]
struct ManifestVersion {
    id: String,
    url: String,
}

#[derive(Deserialize, Serialize)]
struct Version {
    #[serde(rename = "assetIndex")]
    asset_index: VersionAssetIndex,
    downloads: VersionDownloads,
}

#[derive(Deserialize, Serialize)]
struct VersionAssetIndex {
    url: String,
}

#[derive(Deserialize, Serialize)]
struct VersionDownloads {
    client: VersionDownloadsClient,
}

#[derive(Deserialize, Serialize)]
struct VersionDownloadsClient {
    sha1: String,
    url: String,
}

#[derive(Deserialize, Serialize)]
struct AssetIndex {
    objects: HashMap<String, AssetIndexObject>,
}

#[derive(Deserialize, Serialize)]
struct AssetIndexObject {
    hash: String,
}
