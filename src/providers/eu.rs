// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{io::{BufReader, Cursor}, str::FromStr};

use anyhow::bail;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;
use zip::ZipArchive;

use crate::database::SourceUrls;

use super::{DbProvider, DbSource, Downloader, LangId, DbSourceContent, DbTranslation};

// https://joint-research-centre.ec.europa.eu/language-technology-resources_en

pub fn get_eu_source(lang_ids: &[LangId], provider: &DbProvider, _: &Downloader) -> anyhow::Result<()> {
    let urls = [
        SourceUrls {
            originals: None,
            translations: Url::parse("https://wt-public.emm4u.eu/Resources/EAC-TM/EAC-TM-all.zip")?,
        },
        SourceUrls {
            originals: None,
            translations: Url::parse("https://wt-public.emm4u.eu/Resources/ECDC-TM/ECDC-TM.zip")?,
        },
    ];
    for lang_id in lang_ids {
        provider.set_sources(lang_id, &urls)?;
    }
    Ok(())
}

pub fn parse_eu_tmx(source: &DbSource) -> anyhow::Result<()> {
    let en_lang_id = LanguageIdentifier::from_str("en")?;
    let lang_id = source.get_language()?;
    let DbSourceContent::Bytes(bytes) = source.get_contents()?.translations else {
        bail!("No translation bytes");
    };

    let mut translations = Vec::new();

    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    for file_name in zip.file_names().map(|n| n.to_string()).collect::<Vec<_>>() {
        if !file_name.ends_with(".tmx") { continue; }

        let tmx: Tmx = quick_xml::de::from_reader(BufReader::new(zip.by_name(&file_name)?))?;
        for tu in tmx.body.entries {
            let Some(tuv_en) = tu.entries.iter().find(|tuv| tuv.lang == en_lang_id) else {
                continue;
            };
            let Some(tuv) = tu.entries.iter().find(|tuv| tuv.lang == lang_id) else {
                continue;
            };
            translations.push(DbTranslation {
                key: None,
                original: tuv_en.seg.text.clone(),
                translation: tuv.seg.text.clone(),
                comment: None,
            });
        }
    }

    source.set_translations(&translations)?;
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Tmx {
    pub body: Body,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Body {
    #[serde(rename = "tu")]
    pub entries: Vec<Tu>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Tu {
    #[serde(rename = "tuv")]
    pub entries: Vec<Tuv>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Tuv {
    #[serde(rename = "@lang")]
    pub lang: LanguageIdentifier,

    pub seg: Seg,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Seg {
    #[serde(rename = "$text", default)]
    pub text: String,
}
