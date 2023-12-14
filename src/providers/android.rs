use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    sync::Arc,
};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use base64::{
    alphabet::Alphabet,
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use quick_xml::{events::Event, Reader};
use reqwest::{Client, StatusCode};
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

const BASE64: GeneralPurpose = GeneralPurpose::new(
    match &Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/") {
        Ok(alphabet) => alphabet,
        Err(_) => unreachable!(),
    },
    GeneralPurposeConfig::new(),
);

pub struct AndroidProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub decode_as_base64: bool,
    pub default_url: &'static str,
    pub url: F,
}

#[async_trait]
impl<F> TranslationProvider for AndroidProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn group_name(&self) -> Option<&str> {
        self.group_name
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error> {
        let mut translations_all = BTreeMap::new();

        let resources_en =
            get_resources("default", self.default_url, &client, self.decode_as_base64)
                .await?
                .ok_or_else(|| {
                    anyhow!("Default translation were not found\n{}", self.default_url)
                })?;

        'outer: for lang_id in lang_ids {
            let Some(resources) = get_resources(
                &lang_id,
                &(self.url)(&lang_id),
                &client,
                self.decode_as_base64,
            )
            .await?
            else {
                translations_all.insert(lang_id, None);
                continue;
            };
            let mut translations = Vec::with_capacity(resources.len());
            for (name, (text, _)) in resources {
                let Some((text_en, comment_en)) = resources_en.get(&name) else {
                    translations_all.insert(lang_id, None);
                    continue 'outer;
                };
                translations.push(Translation {
                    original: text_en.clone(),
                    translation: text,
                    comment: comment_en.clone(),
                })
            }
            translations_all.insert(lang_id, Some(translations));
        }

        Ok(translations_all)
    }
}

async fn get_resources(
    lang_id: impl Display,
    url: &str,
    client: &Client,
    decode_as_base64: bool,
) -> Result<Option<HashMap<String, (String, Option<String>)>>, anyhow::Error> {
    let response = client.get(url).send().await?;
    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return Ok(None),
        status_code => bail!("Translation '{lang_id}' returned error code '{status_code}'\n{url}",),
    }

    let bytes = response.bytes().await?;
    let bytes = if decode_as_base64 {
        BASE64.decode(&bytes).map_err(|e| {
            anyhow!(
                "Invalid base64 in '{lang_id}' translation: {e}\n{url}\n{}",
                String::from_utf8_lossy(&bytes)
            )
        })?
    } else {
        bytes.to_vec()
    };

    parse_resources(bytes.as_slice()).map(Some)
}

fn parse_resources(
    bytes: &[u8],
) -> Result<HashMap<String, (String, Option<String>)>, anyhow::Error> {
    let mut resources = HashMap::new();

    let mut reader = Reader::from_reader(bytes);
    let mut comment: Option<String> = None;
    let mut name: Option<String> = None;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Err(e) => bail!("{e}"),
            Ok(Event::Eof) => break,
            Ok(Event::Comment(e)) => {
                comment = Some(String::from_utf8_lossy(&e).trim().to_string());
            }
            Ok(Event::Start(e)) if e.name().as_ref() == b"string" => {
                if e.attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"translatable")
                    .map_or(false, |attr| attr.value.as_ref() == b"false")
                {
                    comment = None;
                    continue;
                };
                let Some(name_attr) = e
                    .attributes()
                    .filter_map(|attr| attr.ok())
                    .find(|attr| attr.key.as_ref() == b"name")
                    .and_then(|attr| String::from_utf8(attr.value.to_vec()).ok())
                else {
                    comment = None;
                    continue;
                };
                name = Some(name_attr);
            }
            Ok(Event::Text(e)) if name.is_some() => {
                let t = String::from_utf8_lossy(&e);
                if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
                    text.push_str(&t[1..t.len() - 1]);
                } else {
                    text.push_str(t.trim());
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"string" => {
                let Some(n) = name else {
                    comment = None;
                    text.clear();
                    continue;
                };

                resources.insert(n, (text, comment));

                comment = None;
                name = None;
                text = String::new();
            }
            _ if name.is_some() => comment = None,
            _ => {}
        }
    }

    Ok(resources)
}
