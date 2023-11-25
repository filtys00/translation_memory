use std::{collections::HashMap, sync::Arc};

use anyhow::anyhow;
use async_trait::async_trait;
use log::warn;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use crate::{Translation, TranslationProvider};

pub struct BrowserExtensionProvider<F>
where
    F: Fn(&LanguageIdentifier) -> String + Send + Sync,
{
    pub id: &'static str,
    pub name: &'static str,
    pub group_name: Option<&'static str>,
    pub url: F,
}

#[async_trait]
impl<F> TranslationProvider for BrowserExtensionProvider<F>
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
    ) -> Result<HashMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error> {
        let mut translations = HashMap::new();

        let url = (self.url)(&"en-US".parse()?);
        let messages_en: HashMap<String, Message> = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("{e}\n{url}"))?
            .json()
            .await
            .map_err(|e| anyhow!("{e}\n{url}"))?;

        for lang_id in lang_ids {
            let url = (self.url)(&lang_id);
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(|e| anyhow!("{e}\n{url}"))?;
            match response.status() {
                StatusCode::OK => {}
                StatusCode::NOT_FOUND => continue,
                status_code => {
                    warn!("Unexpected status code: {status_code}\n{url}");
                    continue;
                }
            }
            let messages: HashMap<String, Message> =
                response.json().await.map_err(|e| anyhow!("{e}\n{url}"))?;

            let mut t = Vec::with_capacity(messages.len());
            for (key, message) in messages {
                let Some(message_en) = messages_en.get(&key) else {
                    warn!("Translation key '{key}' were found in '{lang_id}' translation but not in 'en' translation");
                    continue;
                };

                t.push(Translation {
                    original: message_en.message.clone(),
                    translation: message.message,
                    comment: if let Some(placeholders) = message.placeholders {
                        Some(
                            placeholders
                                .iter()
                                .fold(key, |mut acc, (name, placeholder)| {
                                    acc = acc + "\n" + name + ": " + &placeholder.content;
                                    if let Some(example) = &placeholder.example {
                                        acc + "\n    " + example
                                    } else {
                                        acc
                                    }
                                }),
                        )
                    } else {
                        Some(key)
                    },
                });
            }
            translations.insert(lang_id, Some(t));
        }

        Ok(translations)
    }
}

#[derive(Deserialize, Serialize)]
struct Message {
    message: String,
    placeholders: Option<HashMap<String, Placeholder>>,
}

#[derive(Deserialize, Serialize)]
struct Placeholder {
    content: String,
    example: Option<String>,
}
