// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{anyhow, bail};
use unic_langid::LanguageIdentifier;

use crate::Translation;

pub fn parse_mozilla_tmx(text: String, source: &str) -> anyhow::Result<Vec<Translation>> {
    let tmx: tmx::Tmx = quick_xml::de::from_str(&text)?;

    let lang_id = tmx
        .body
        .entries
        .iter()
        .find_map(|tu| {
            let tuv = tu.entries.iter().find(|tuv| tuv.lang != tu.srclang);
            tuv.map(|tuv| &tuv.lang)
        })
        .ok_or_else(|| anyhow!("Could not find any language other than the source"))?;
    let lang_ids: Vec<&LanguageIdentifier> = tmx
        .body
        .entries
        .iter()
        .flat_map(|tu| {
            tu.entries
                .iter()
                .filter(|tuv| tuv.lang != tu.srclang && tuv.lang != *lang_id)
        })
        .map(|tuv| &tuv.lang)
        .collect();
    if !lang_ids.is_empty() {
        bail!(
            "Found two different languages other than the source: {}",
            lang_ids
                .iter()
                .map(|lang_id| lang_id.to_string())
                .reduce(|acc, lang_id| acc + ", " + &lang_id)
                .unwrap_or_default()
        );
    }

    let translations = tmx
        .body
        .entries
        .iter()
        .filter_map(|tu| {
            let tuv_en = tu.entries.iter().find(|tuv| tuv.lang == tu.srclang)?;
            let tuv = tu.entries.iter().find(|tuv| tuv.lang == *lang_id)?;

            Some(Translation {
                original: tuv_en.seg.text.clone(),
                translation: tuv.seg.text.clone(),
                comment: None,
                key: Some(tu.tuid.clone()),
                source: source.to_string(),
            })
        })
        .collect();

    Ok(translations)
}

mod tmx {
    use serde::{Deserialize, Serialize};
    use unic_langid::LanguageIdentifier;

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
        #[serde(rename = "@tuid")]
        pub tuid: String,
        #[serde(rename = "@srclang")]
        pub srclang: LanguageIdentifier,

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
        #[serde(rename = "$text")]
        pub text: String,
    }
}

pub fn parse_mozilla_tbx(text: String, source: &str) -> anyhow::Result<Vec<Translation>> {
    let tbx: tbx::Tbx = quick_xml::de::from_str(&text)?;

    let en_us: LanguageIdentifier = "en-US".parse().unwrap();
    let lang_id = tbx
        .text
        .body
        .entries
        .iter()
        .find_map(|concept_entry| {
            let lang_sec = concept_entry
                .entries
                .iter()
                .find(|lang_sec| lang_sec.lang != en_us);
            lang_sec.map(|lang_sec| &lang_sec.lang)
        })
        .ok_or_else(|| anyhow!("Could not find any language other than '{en_us}'"))?;
    let lang_ids: Vec<&LanguageIdentifier> = tbx
        .text
        .body
        .entries
        .iter()
        .flat_map(|concept_entry| {
            concept_entry
                .entries
                .iter()
                .filter(|lang_sec| lang_sec.lang != en_us && lang_sec.lang != *lang_id)
        })
        .map(|lang_sec| &lang_sec.lang)
        .collect();
    if !lang_ids.is_empty() {
        bail!(
            "Found two different languages other than source: {}",
            lang_ids
                .iter()
                .map(|lang_id| lang_id.to_string())
                .reduce(|acc, lang_id| acc + ", " + &lang_id)
                .unwrap_or_default()
        );
    }

    let translations = tbx
        .text
        .body
        .entries
        .iter()
        .filter_map(|concept_entry| {
            let lang_sec_en = concept_entry
                .entries
                .iter()
                .find(|lang_sec| lang_sec.lang == en_us)?;
            let lang_sec = concept_entry
                .entries
                .iter()
                .find(|lang_sec| lang_sec.lang == *lang_id)?;

            Some(Translation {
                original: lang_sec_en.term_sec.term.text.clone(),
                translation: lang_sec.term_sec.term.text.clone(),
                comment: lang_sec_en
                    .term_sec
                    .descrip_grp
                    .as_ref()
                    .and_then(|descrip_grp| {
                        descrip_grp
                            .entries
                            .iter()
                            .filter(|descrip| !descrip.text.is_empty())
                            .map(|descrip| &descrip.text)
                            .cloned()
                            .reduce(|acc, descrip| acc + "\n" + &descrip)
                    }),
                key: None,
                source: source.to_string(),
            })
        })
        .collect();

    Ok(translations)
}

mod tbx {
    use serde::{Deserialize, Serialize};
    use unic_langid::LanguageIdentifier;

    #[derive(Debug, Deserialize, Serialize)]
    pub struct Tbx {
        pub text: Text,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct Text {
        pub body: Body,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct Body {
        #[serde(rename = "conceptEntry")]
        pub entries: Vec<ConceptEntry>,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct ConceptEntry {
        #[serde(rename = "langSec")]
        pub entries: Vec<LangSec>,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct LangSec {
        #[serde(rename = "@lang")]
        pub lang: LanguageIdentifier,

        #[serde(rename = "termSec")]
        pub term_sec: TermSec,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct TermSec {
        pub term: Term,
        #[serde(rename = "descripGrp")]
        pub descrip_grp: Option<DescripGrp>,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct Term {
        #[serde(rename = "$text")]
        pub text: String,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct DescripGrp {
        #[serde(rename = "descrip")]
        pub entries: Vec<Descrip>,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct Descrip {
        #[serde(rename = "@type")]
        pub descrip_type: String,

        #[serde(rename = "$text", default)]
        pub text: String,
    }
}
