use std::{collections::HashMap, io::Cursor, sync::Arc};

use anyhow::anyhow;
use async_trait::async_trait;
use log::trace;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;
use zip::ZipArchive;

use crate::{Translation, TranslationProvider};

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
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<HashMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error> {
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

        let mut translations = HashMap::new();

        let assets_en: HashMap<String, String> = {
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
            let Some(region) = lang_id.region else {
                translations.insert(lang_id, None);
                continue;
            };
            let key = format!(
                "minecraft/lang/{}_{}.json",
                lang_id.language.as_str(),
                region.as_str().to_lowercase(),
            );

            let Some(object) = asset_index.objects.get(&key) else {
                translations.insert(lang_id, None);
                continue;
            };
            let assets: HashMap<String, String> = client
                .get(&format!(
                    "https://resources.download.minecraft.net/{}/{}",
                    object.hash.get(0..2).unwrap_or(""),
                    object.hash
                ))
                .send()
                .await?
                .json()
                .await?;

            let mut t = Vec::with_capacity(assets.len());
            for (key, value) in assets {
                let Some(original) = assets_en.get(&key) else {
                    trace!("Translation key '{key}' were found in '{lang_id}' translation but not in default translation");
                    continue;
                };

                t.push(Translation {
                    original: original.clone(),
                    translation: value.clone(),
                    comment: Some(key),
                });
            }
            translations.insert(lang_id, Some(t));
        }

        Ok(translations)
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
