// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod android;
mod browser_extension;
mod chrome;
mod defaults;
mod dtd;
mod json;
mod minecraft;
mod mozilla;
mod po;
mod properties;
mod srt;

use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
    sync::Arc,
};

use async_trait::async_trait;
use reqwest::Client;
use unic_langid::LanguageIdentifier;

pub use self::defaults::default_providers;
use super::Translation;

#[async_trait]
pub trait TranslationProvider {
    fn id(&self) -> &str;

    fn name(&self) -> &str;

    fn group_name(&self) -> Option<&str> {
        None
    }

    async fn generate(
        &self,
        lang_ids: Vec<LanguageIdentifier>,
        client: Arc<Client>,
    ) -> Result<BTreeMap<LanguageIdentifier, Option<Vec<Translation>>>, anyhow::Error>;
}

impl Debug for dyn TranslationProvider + Send + Sync {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "provider({})", self.id())
    }
}

/// Returns a string version of `lang_id`.
///
/// # Examples
/// ```ignore
/// assert!(
///     lang_id_to_string("ca_ES_valencia".parse().unwrap(), "-", false, "@", true),
///     String::from("ca-es@VALENCIA"),
/// );
/// ```
fn lang_id_to_string(
    lang_id: &LanguageIdentifier,
    region_binder: &str,
    uppercase_region: bool,
    variant_binder: &str,
    uppercase_variant: bool,
) -> String {
    let mut s = lang_id.language.to_string();
    if let Some(region) = &lang_id.region {
        s.push_str(region_binder);
        if uppercase_region {
            s.push_str(region.as_str());
        } else {
            s.push_str(&region.as_str().to_lowercase());
        }
    }
    for variant in lang_id.variants() {
        s.push_str(variant_binder);
        if uppercase_variant {
            s.push_str(&variant.as_str().to_uppercase());
        } else {
            s.push_str(variant.as_str());
        }
    }
    s
}
