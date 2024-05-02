// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use crate::Translation;

pub fn parse_microsoft_tbx(text: String, source: &str) -> anyhow::Result<Vec<Translation>> {
    let translations: MicrosoftTranslations = quick_xml::de::from_str(&text)?;

    let en_us: LanguageIdentifier = "en-US".parse()?;
    let lang_id = translations
        .text
        .body
        .term_entries
        .iter()
        .find_map(|term_entry| {
            let lang_set = term_entry
                .lang_sets
                .iter()
                .find(|lang_set| lang_set.lang != en_us);
            lang_set.map(|lang_set| &lang_set.lang)
        })
        .ok_or_else(|| anyhow!("Could not find any language other than '{en_us}'"))?;
    let lang_ids: Vec<&LanguageIdentifier> = translations
        .text
        .body
        .term_entries
        .iter()
        .flat_map(|term_entry| &term_entry.lang_sets)
        .map(|lang_set| &lang_set.lang)
        .filter(|lang| **lang != en_us && *lang != lang_id)
        .collect();
    if !lang_ids.is_empty() {
        bail!(
            "Found two different languages other than '{en_us}': {}",
            lang_ids
                .iter()
                .map(|lang_id| lang_id.to_string())
                .reduce(|acc, lang_id| acc + ", " + &lang_id)
                .unwrap_or_default()
        );
    }

    let translations = translations
        .text
        .body
        .term_entries
        .iter()
        .filter_map(|term_entry| {
            let term_entry_en = term_entry
                .lang_sets
                .iter()
                .find(|lang_set| lang_set.lang == en_us)?;
            let term_entry = term_entry
                .lang_sets
                .iter()
                .find(|lang_set| lang_set.lang == *lang_id)?;

            Some(Translation {
                original: term_entry_en.ntig.term_grp.term.text.clone(),
                translation: term_entry.ntig.term_grp.term.text.clone(),
                comment: term_entry_en
                    .descrip_grp
                    .as_ref()
                    .map(|descrip_grp| descrip_grp.descrip.text.clone()),
                key: None,
                source: source.to_string(),
            })
        })
        .collect();

    Ok(translations)
}

#[derive(Debug, Deserialize, Serialize)]
struct MicrosoftTranslations {
    text: Text,
}

#[derive(Debug, Deserialize, Serialize)]
struct Text {
    body: Body,
}

#[derive(Debug, Deserialize, Serialize)]
struct Body {
    #[serde(rename = "termEntry")]
    term_entries: Vec<TermEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TermEntry {
    // #[serde(rename = "@id")]
    // id: String,
    #[serde(rename = "langSet")]
    lang_sets: Vec<LangSet>,
}

#[derive(Debug, Deserialize, Serialize)]
struct LangSet {
    #[serde(rename = "@lang")]
    lang: LanguageIdentifier,

    #[serde(rename = "descrip_grp")]
    descrip_grp: Option<DescripGrp>,

    ntig: Ntig,
}

#[derive(Debug, Deserialize, Serialize)]
struct DescripGrp {
    descrip: Descrip,
}

#[derive(Debug, Deserialize, Serialize)]
struct Descrip {
    // #[serde(rename = "@type")]
    // type_attr: String,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Ntig {
    #[serde(rename = "termGrp")]
    term_grp: TermGrp,
}

#[derive(Debug, Deserialize, Serialize)]
struct TermGrp {
    term: Term,
    // #[serde(rename = "termNote")]
    // term_notes: Vec<TermNote>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Term {
    // #[serde(rename = "@id")]
    // id: u64,
    #[serde(rename = "$text")]
    text: String,
}

// #[derive(Debug, Deserialize, Serialize)]
// struct TermNote {
//     #[serde(rename = "@type")]
//     type_attr: String,

//     #[serde(rename = "$text")]
//     text: String,
// }
