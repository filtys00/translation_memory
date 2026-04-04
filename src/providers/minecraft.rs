// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{collections::HashMap, io::Cursor};

use anyhow::{anyhow, bail};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use super::{
    DbProvider,
    DbSource,
    DbSourceContent,
    DbSourceUrls,
    Downloader,
    LangId,
    TranslationMessages,
    merge_messages,
};

pub fn get_minecraft_sources(lang_ids: &[LangId], provider: &DbProvider, downloader: &Downloader) -> anyhow::Result<()> {
    let manifest: Manifest = downloader.get_json("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")?;
    let version = manifest
        .versions
        .iter()
        .find(|version| version.id == manifest.latest.release)
        .ok_or_else(|| {
            anyhow!("Malformed Minecraft manifest: 'latest.release' does not exist")
        })?;
    let version: Version = downloader.get_json(&version.url)?;
    let asset_index: AssetIndex = downloader.get_json(&version.asset_index.url)?;

    let default_url = Url::parse(&version.downloads.client.url)?;

    for lang_id in lang_ids {
        let key = format!("minecraft/lang/{}.json", lang_id.format("_", false, "_", false));
        let Some(object) = asset_index.objects.get(&key) else { continue; };
        let url = Url::parse(&format!(
            "https://resources.download.minecraft.net/{}/{}",
            object.hash.get(0..2).unwrap_or(""),
            object.hash,
        ))?;
        provider.set_sources(lang_id, &[
            DbSourceUrls { originals: Some(default_url.clone()), translations: url }
        ])?;
    }

    Ok(())
}

fn parse_content(content: DbSourceContent) -> anyhow::Result<TranslationMessages> {
    let messages: HashMap<String, String> = match content {
        DbSourceContent::None => bail!("No source content"),
        DbSourceContent::Text(text ) => serde_json::from_str(&text)?,
        DbSourceContent::Bytes(bytes) => {
            let mut zip = ZipArchive::new(Cursor::new(bytes))?;
            let lang_file = zip.by_name("assets/minecraft/lang/en_us.json")?;
            serde_json::from_reader(lang_file)?
        }
    };
    let messages = messages.into_iter()
        .map(|(key, translation)| (key, (translation, None)))
        .collect();
    Ok(messages)
}

pub fn parse_minecraft(source: &DbSource) -> anyhow::Result<()> {
    let contents = source.get_contents()?;
    let default_messages = parse_content(contents.originals)?;
    let messages = parse_content(contents.translations)?;
    let translations = merge_messages(messages, default_messages);
    source.set_translations(&translations)?;
    Ok(())
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
