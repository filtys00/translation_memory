// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use async_trait::async_trait;
use log::trace;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use super::lang_id_to_string;
use crate::{Translation, TranslationProvider};

pub struct MozillaProvider;

#[async_trait]
impl TranslationProvider for MozillaProvider {
    fn id(&self) -> &str {
        "mozilla"
    }

    fn name(&self) -> &str {
        "Mozilla"
    }

    fn group_name(&self) -> Option<&str> {
        None
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error> {
        let mut translations_all = BTreeMap::new();

        for lang_id in lang_ids {
            client
                .get("https://transvision.mozfr.org/downloads/")
                .query(&[("locale", &lang_id)])
                .query(&[
                    ("tmx_format", "normal"),
                    ("gecko_strings", "gecko_strings"),
                    ("android_l10n", "android_l10n"),
                    ("firefox_ios", "firefox_ios"),
                    ("mozilla_org", "mozilla_org"),
                    ("vpn_client", "vpn_client"),
                    ("comm_l10n", "comm_l10n"),
                ])
                .send()
                .await?;
            let url = format!(
                "https://transvision.mozfr.org/download/mozilla_en-US_{}_523b2e5104b223d95d89461163351877_normal.tmx",
                lang_id_to_string(&lang_id, "-", true, "-", false),
            );
            let bytes = client.get(&url).send().await?.bytes().await?;
            if bytes.is_empty() {
                trace!("Skipping '{url}' as it is empty");
                translations_all.insert(lang_id, None);
                continue;
            }
            let translations: Tbx = quick_xml::de::from_reader(Cursor::new(bytes))?;

            let en_us: LanguageIdentifier = "en-US".parse()?;
            let translations = translations
                .body
                .tu
                .iter()
                .filter_map(|tu| {
                    let tuv_en = tu.tuv.iter().find(|tuv| tuv.lang == en_us)?;
                    let tuv = tu.tuv.iter().find(|tuv| tuv.lang == lang_id)?;

                    Some(Translation {
                        original: tuv_en.seg.clone(),
                        translation: tuv.seg.clone(),
                        comment: Some(tu.id.clone()),
                        key: None,
                        source: url.clone(),
                    })
                })
                .collect();

            translations_all.insert(lang_id, Some(translations));
        }

        Ok(translations_all)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Tbx {
    // #[serde(rename = "@version")]
    // version: String,
    body: Body,
}

#[derive(Debug, Deserialize, Serialize)]
struct Body {
    tu: Vec<Tu>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Tu {
    #[serde(rename = "@tuid")]
    id: String,
    // #[serde(rename = "@srclang")]
    // src_lang: LanguageIdentifier,
    tuv: Vec<Tuv>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Tuv {
    #[serde(rename = "@lang")]
    lang: LanguageIdentifier,

    seg: String,
}
