// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::BTreeMap,
    io::{BufReader, Cursor},
    sync::Arc,
};

use anyhow::anyhow;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;
use zip::{read::ZipFile, ZipArchive};

use super::TranslationProvider;
use crate::{ProviderCache, ProviderCacheMultiple, Translation, TranslationBundle};

// https://joint-research-centre.ec.europa.eu/language-technology-resources_en

pub struct EuProvider;

#[async_trait]
impl TranslationProvider for EuProvider {
    fn id(&self) -> &str {
        "eu"
    }

    fn name(&self) -> &str {
        "European Commision"
    }

    async fn generate(
        &self,
        _previous: Option<ProviderCacheMultiple>,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> anyhow::Result<ProviderCache> {
        fn parse_tmx(
            zip_file: ZipFile,
            lang_ids: &[LanguageIdentifier],
            translation_bundle: &mut TranslationBundle,
            en: &LanguageIdentifier,
            url: &str,
        ) -> anyhow::Result<()> {
            let tmx: Tmx = quick_xml::de::from_reader(BufReader::new(zip_file))?;
            for tu in tmx.body.entries {
                let Some(tuv_en) = tu.entries.iter().find(|tuv| tuv.lang == *en) else {
                    continue;
                };
                for lang_id in lang_ids {
                    let Some(tuv) = tu.entries.iter().find(|tuv| tuv.lang == *lang_id) else {
                        continue;
                    };
                    let Some(translations) = translation_bundle
                        .entry(lang_id.clone())
                        .or_insert_with(|| Some(Vec::new()))
                    else {
                        continue;
                    };
                    translations.push(Translation {
                        original: tuv_en.seg.text.clone(),
                        translation: tuv.seg.text.clone(),
                        comment: None,
                        key: None,
                        source: url.to_string(),
                    });
                }
            }
            Ok(())
        }

        let en: LanguageIdentifier = "en".parse().unwrap();
        let mut translation_bundle = BTreeMap::new();

        let url = "https://wt-public.emm4u.eu/Resources/EAC-TM/EAC-TM-all.zip";
        let response = client.get(url).send().await?;
        let bytes = response.bytes().await?;
        let mut zip = ZipArchive::new(Cursor::new(bytes))?;
        parse_tmx(
            zip.by_name("EAC_REFRENCE_DATA.tmx")
                .map_err(|e| anyhow!("could not get file 'EAC_REFRENCE_DATA.tmx': {e}"))?,
            &lang_ids,
            &mut translation_bundle,
            &en,
            url,
        )
        .map_err(|e| anyhow!("could not parse file 'EAC_REFRENCE_DATA.tmx': {e}"))?;
        parse_tmx(
            zip.by_name("EAC_FORMS.tmx")
                .map_err(|e| anyhow!("could not get file 'EAC_FORMS.tmx': {e}"))?,
            &lang_ids,
            &mut translation_bundle,
            &en,
            url,
        )
        .map_err(|e| anyhow!("could not parse file 'EAC_FORMS.tmx': {e}"))?;

        let url = "https://wt-public.emm4u.eu/Resources/ECDC-TM/ECDC-TM.zip";
        let response = client.get(url).send().await?;
        let bytes = response.bytes().await?;
        let mut zip = ZipArchive::new(Cursor::new(bytes))?;
        parse_tmx(
            zip.by_name("ECDC-TM/ECDC.tmx")
                .map_err(|e| anyhow!("could not get file 'ECDC-TM/ECDC.tmx': {e}"))?,
            &lang_ids,
            &mut translation_bundle,
            &en,
            url,
        )
        .map_err(|e| anyhow!("could not parse file 'ECDC-TM/ECDC.tmx': {e}"))?;

        Ok(ProviderCache::Single(translation_bundle))
    }
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
