// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use super::{Downloader, LangId};

pub fn crawl_libreoffice(lang_id: &LangId, downloader: &Downloader) -> anyhow::Result<Vec<String>> {
    let mut urls = Vec::new();

    crawl(&mut urls, "", lang_id, downloader)?;

    Ok(urls)
}

fn crawl(
    urls: &mut Vec<String>,
    path: &str,
    lang_id: &LangId,
    downloader: &Downloader,
) -> anyhow::Result<()> {
    let url = format!(
        "https://git.libreoffice.org/translations/+/refs/heads/master/source/{}{path}?format=JSON",
        lang_id.format("-", true, "-", false),
    );
    let Some(response) = downloader.get_text(&url)? else { return Ok(()); };
    let response: String = response.lines().skip(1).collect();
    let response: Tree = serde_json::from_str(&response)
        .map_err(|e| anyhow!("Could not parse as JSON (skipping first line): {e}\n{url}"))?;

    for entry in response.entries {
        match entry.entry_type {
            EntryType::Blob => {
                urls.push(format!(
                    "https://git.libreoffice.org/translations/+/refs/heads/master/source/{}{path}/{}?format=TEXT",
                    lang_id.format("-", true, "-", false),
                    entry.name,
                ));
            }
            EntryType::Tree => {
                crawl(
                    urls,
                    &format!("{path}/{}", entry.name),
                    lang_id,
                    downloader,
                )?;
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
