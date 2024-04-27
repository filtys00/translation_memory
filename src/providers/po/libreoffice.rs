// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::sync::Arc;

use anyhow::anyhow;
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use crate::providers::lang_id_to_string;

pub async fn crawl_libreoffice(
    lang_id: LanguageIdentifier,
    client: Arc<Client>,
) -> Result<Vec<String>, anyhow::Error> {
    let mut urls = Vec::new();

    crawl(&mut urls, "", &lang_id, &client).await?;

    Ok(urls)
}

async fn crawl(
    urls: &mut Vec<String>,
    path: &str,
    lang_id: &LanguageIdentifier,
    client: &Client,
) -> anyhow::Result<()> {
    let url = format!(
        "https://git.libreoffice.org/translations/+/refs/heads/master/source/{}{path}?format=JSON",
        lang_id_to_string(lang_id, "-", true, "-", false),
    );
    let response: Response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("Could not send request: {e}\n{url}"))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(());
    }
    let response = response
        .text()
        .await
        .map_err(|e| anyhow!("Could not parse as text: {e}\n{url}"))?;
    let response: String = response.lines().skip(1).collect();
    let response: Tree = serde_json::from_str(&response)
        .map_err(|e| anyhow!("Could not parse as JSON (skipping first line): {e}\n{url}"))?;

    for entry in response.entries {
        match entry.entry_type {
            EntryType::Blob => {
                urls.push(format!(
                    "https://cgit.freedesktop.org/libreoffice/translations/plain/source/{}{path}/{}",
                    lang_id_to_string(lang_id, "-", true, "-", false),
                    entry.name,
                ));
            }
            EntryType::Tree => {
                Box::pin(crawl(
                    urls,
                    &format!("{path}/{}", entry.name),
                    lang_id,
                    client,
                ))
                .await?;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct Tree {
    id: String,
    entries: Vec<Entry>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Entry {
    mode: i32,
    #[serde(rename = "type")]
    entry_type: EntryType,
    id: String,
    name: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum EntryType {
    Tree,
    Blob,
}
