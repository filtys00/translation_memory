// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{cell::RefCell, collections::HashSet, str::FromStr};

use regex::Regex;
use rusqlite::{
    functions::FunctionFlags,
    types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, Type as SqlType, ValueRef},
    Connection,
    OptionalExtension,
    Error as SqlError,
    Result as SqlResult,
};
use unic_langid::LanguageIdentifier;

/// SQL to initialize the SQLite database.
const INIT_SQL: &str = r#"
CREATE TABLE Languages (
    id INTEGER PRIMARY KEY,
    code TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE Providers (
    id INTEGER PRIMARY KEY,
    type                  TEXT NOT NULL,
    code                  TEXT NOT NULL UNIQUE,
    name                  TEXT NOT NULL,
    group_name            TEXT,
    sources_download_time INTEGER,
    has_failed            INTEGER NOT NULL DEFAULT 0,
    CHECK type in ("builtin", "retired", "from_file")
) STRICT;

CREATE TABLE Sources (
    id INTEGER PRIMARY KEY,

    provider_id INTEGER NOT NULL,
    language_id INTEGER NOT NULL,

    originals_url     TEXT UNIQUE,
    translations_url  TEXT NOT NULL UNIQUE,

    download_time     INTEGER,
    originals_text    TEXT,
    translations_text TEXT,

    has_failed        INTEGER NOT NULL DEFAULT 0,

    FOREIGN KEY (provider_id) REFERENCES Providers(id),
    FOREIGN KEY (language_id) REFERENCES Languages(id)
) STRICT;

CREATE TABLE Translations (
    id INTEGER PRIMARY KEY,

    source_id   INTEGER NOT NULL,

    key         TEXT,
    original    TEXT NOT NULL,
    translation TEXT NOT NULL,
    comment     TEXT,

    FOREIGN KEY (source_id)   REFERENCES Sources(id)
) STRICT;

CREATE INDEX Translations_SourceId ON Translations (source_id);
CREATE INDEX Sources_ProviderId ON Sources (provider_id);
"#;

/// A connection to a translation database.
pub struct TranslationStore { connection: RefCell<Connection> }

/// An independent provider of translation sources.
pub struct Provider<'a> { connection: &'a RefCell<Connection>, id: i64 }

/// An independent source of translations.
pub struct Source<'a> { connection: &'a RefCell<Connection>, id: i64 }

/// Why a provider were created.
#[derive(PartialEq, Eq, Hash)]
pub enum ProviderType {
    /// A provider that is predefined.
    BuiltIn,
    /// A provider that used to be predefined.
    Retired,
    /// A provider that is manually added from a file.
    FromFile,
}

impl FromSql for ProviderType {
    fn column_result(value: ValueRef) -> FromSqlResult<Self> {
        match value.as_str()? {
            "builtin"   => Ok(ProviderType::BuiltIn),
            "retired"   => Ok(ProviderType::Retired),
            "from_file" => Ok(ProviderType::FromFile),
            _ => Err(FromSqlError::InvalidType)
        }
    }
}

impl ToSql for ProviderType {
    fn to_sql(&self) -> SqlResult<ToSqlOutput<'_>> {
        let provider_type = match self {
            ProviderType::BuiltIn => "builtin",
            ProviderType::Retired => "retired",
            ProviderType::FromFile => "from_file",
        };
        Ok(provider_type.into())
    }
}

#[derive(PartialEq, Eq, Hash)]
pub struct ProviderNames {
    pub code: String,
    pub name: String,
    pub group_name: Option<String>,
}

pub struct SourceUrls {
    pub originals_url: Option<String>,
    pub translations_url: String,
}

pub struct SourceTexts {
    pub originals_text: Option<String>,
    pub translations_text: Option<String>,
}

pub struct Translation {
    pub key: Option<String>,
    pub original: String,
    pub translation: String,
    pub comment: Option<String>,
}

impl TranslationStore {
    /// Open `connection` as a translation database, initiating it if necessary.
    pub fn open(connection: Connection) -> SqlResult<Self> {
        let has_providers_table = connection.table_exists(None, "Providers")?;
        if !has_providers_table { connection.execute_batch(INIT_SQL)?; }

        let function_flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
        connection.create_scalar_function("regexp", 2, function_flags, |context| {
            let regex = context.get_or_create_aux(0, |value| -> SqlResult<Regex> {
                let regex = Regex::new(value.as_str()?)
                    .map_err(|e| SqlError::FromSqlConversionFailure(0, SqlType::Text, Box::new(e)))?;
                Ok(regex)
            })?;
            let value = context.get_raw(1).as_str()?;
            Ok(regex.is_match(value))
        })?;

        Ok(Self { connection: RefCell::new(connection) })
    }

    /// Returns a set of all the language codes.
    pub fn get_languages(&self) -> SqlResult<HashSet<LanguageIdentifier>> {
        let languages = self.connection.borrow()
            .prepare("SELECT code FROM Languages")?
            .query_map((), |row| {
                LanguageIdentifier::from_str(row.get_ref(0)?.as_str()?).map_err(|e| {
                    SqlError::FromSqlConversionFailure(0, SqlType::Text, Box::new(e))
                })
            })?
            .collect::<SqlResult<_>>()?;
        Ok(languages)
    }

    /// Returns a list of all the providers.
    pub fn get_providers(&'_ self) -> SqlResult<Vec<Provider<'_>>> {
        let providers = self.connection.borrow().prepare("SELECT id FROM Providers")?
            .query_map((), |row| {
                let provider = Provider { connection: &self.connection, id: row.get(0)? };
                Ok(provider)
            })?
            .collect::<SqlResult<_>>()?;
        Ok(providers)
    }

    /// Returns the provider with code name `code`, or `None` if it does not exist.
    pub fn get_provider(&'_ self, code: &str) -> SqlResult<Option<Provider<'_>>> {
        let provider = self.connection.borrow()
            .query_one("SELECT id FROM Providers WHERE code = ?", [code], |row| {
                Ok(Provider { connection: &self.connection, id: row.get(0)? })
            })
            .optional()?;
        Ok(provider)
    }

    /// Adds a new provider.
    pub fn add_provider(&'_ self, provider_type: ProviderType, names: ProviderNames) -> SqlResult<Provider<'_>> {
        let connection = self.connection.borrow();
        connection.execute(
            "INSERT INTO Providers (type, code, name, group_name) VALUES (?, ?, ?, ?)",
            (provider_type, names.code, names.name, names.group_name),
        )?;
        let provider_id = connection.last_insert_rowid();
        Ok(Provider { connection: &self.connection, id: provider_id })
    }
}

impl Provider<'_> {
    /// Returns the type of this provider.
    pub fn get_type(&self) -> SqlResult<ProviderType> {
        let provider_type = self.connection.borrow().query_one(
            "SELECT type FROM Providers WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(provider_type)
    }

    /// Sets the type of this provider.
    pub fn set_type(&self, provider_type: ProviderType) -> SqlResult<()> {
        self.connection.borrow()
            .execute("UPDATE Providers SET type = ? WHERE id = ?", (provider_type, self.id))?;
        Ok(())
    }

    /// Returns whether the provider has `failed`.
    pub fn get_names(&self) -> SqlResult<ProviderNames> {
        let failed = self.connection.borrow().query_one(
            "SELECT code, name, group_name FROM Providers WHERE id = ?", [self.id],
            |row| {
                let names = ProviderNames {
                    code: row.get(0)?,
                    name: row.get(1)?,
                    group_name: row.get(2)?,
                };
                Ok(names)
            }
        )?;
        Ok(failed)
    }

    /// Sets whether the provider has `failed`.
    pub fn set_names(&self, name: &str, group_name: Option<&str>) -> SqlResult<()> {
        self.connection.borrow().execute(
            "UPDATE Providers SET name = ?, group_name = ? WHERE id = ?",
            (name, group_name, self.id),
        )?;
        Ok(())
    }

    /// Returns whether the provider has `failed`.
    pub fn has_failed(&self) -> SqlResult<bool> {
        let failed = self.connection.borrow().query_one(
            "SELECT has_failed FROM Providers WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(failed)
    }

    /// Sets whether the provider has `failed`.
    pub fn set_failed(&self, failed: bool) -> SqlResult<()> {
        self.connection.borrow()
            .execute("UPDATE Providers SET has_failed = ? WHERE id = ?", (failed, self.id))?;
        Ok(())
    }

    /// Returns the sources download time of this provider.
    pub fn get_sources_download_time(&self) -> SqlResult<bool> {
        let sources_download_time = self.connection.borrow().query_one(
            "SELECT sources_download_time FROM Providers WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(sources_download_time)
    }

    /// Sets the sources download time of this provider.
    pub fn set_sources_download_time(&self, sources_download_time: Option<u32>) -> SqlResult<()> {
        self.connection.borrow().execute(
            "UPDATE Providers SET sources_download_time = ? WHERE id = ?",
            (sources_download_time, self.id),
        )?;
        Ok(())
    }

    /// Returns a list of all the sources that are associated with this provider.
    pub fn get_sources(&'_ self) -> SqlResult<Vec<Source<'_>>> {
        let sources = self.connection.borrow()
            .prepare("SELECT id FROM Sources WHERE provider_id = ?")?
            .query_map([self.id], |row| {
                let provider = Source { connection: self.connection, id: row.get(0)? };
                Ok(provider)
            })?
            .collect::<SqlResult<_>>()?;
        Ok(sources)
    }

    /// Adds a source to this provider with `language_code` and `urls`.
    pub fn add_source(&'_ self, language_code: &LanguageIdentifier, urls: SourceUrls) -> SqlResult<Source<'_>> {
        let connection = self.connection.borrow();
        let language_id: Option<i64> = connection
            .query_one(
                "SELECT id FROM Languages WHERE code = ?", [language_code.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if language_id.is_none() {
            connection.execute("INSERT INTO Languages (code) VALUES (?)", [language_code.to_string()])?;
        }
        let language_id = if let Some(id) = language_id { id } else { connection.last_insert_rowid() };

        connection.execute(
            "INSERT INTO Sources (provider_id, language_id, originals_url, translations_url) VALUES (?, ?, ?, ?)",
            (self.id, language_id, urls.originals_url, urls.translations_url),
        )?;
        let source_id = connection.last_insert_rowid();
        Ok(Source { connection: self.connection, id: source_id })
    }

    /// Deletes all sources that belongs to this provider. 
    pub fn clear_sources(&self) -> SqlResult<()> {
        self.connection.borrow().execute("DELETE FROM Sources WHERE provider_id = ?", [self.id])?;
        Ok(())
    }
}

impl Source<'_> {
    /// Returns the urls of this source.
    pub fn get_urls(&self) -> SqlResult<SourceUrls> {
        let urls = self.connection.borrow().query_one(
            "SELECT originals_url, translations_url FROM Sources WHERE id = ?", [self.id],
            |row| Ok(SourceUrls { originals_url: row.get(0)?, translations_url: row.get(1)? }),
        )?;
        Ok(urls)
    }

    /// Returns the download time in unix time, and the downloaded texts of this source.
    pub fn get_text(&self) -> SqlResult<(u32, SourceTexts)> {
        let (download_time, texts) = self.connection.borrow().query_one(
            "SELECT download_time, originals_text, translations_text FROM Sources WHERE id = ?", [self.id],
            |row| {
                let download_time = row.get(0)?;
                let texts = SourceTexts {
                    originals_text: row.get(1)?,
                    translations_text: row.get(2)?,
                };
                Ok((download_time, texts))
            }
        )?;
        Ok((download_time, texts))
    }

    /// Sets the downloaded texts to this source, using the current time as download time.
    pub fn set_text(&self, texts: SourceTexts) -> SqlResult<()> {
        self.connection.borrow().execute(
            "UPDATE Sources SET download_time = unixepoch(), originals_text = ?, translations_text = ? WHERE id = ?",
            (texts.originals_text, texts.translations_text, self.id),
        )?;
        Ok(())
    }

    /// Returns whether this source has `failed`.
    pub fn has_failed(&self) -> SqlResult<bool> {
        let failed = self.connection.borrow().query_one(
            "SELECT has_failed FROM Sources WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(failed)
    }

    /// Sets whether this source has `failed`.
    pub fn set_failed(&self, failed: bool) -> SqlResult<()> {
        self.connection.borrow()
            .execute("UPDATE Sources SET has_failed = ? WHERE id = ?", (failed, self.id))?;
        Ok(())
    }

    /// Sets the translations associated with this source.
    pub fn set_translations(&self, translations: Vec<Translation>) -> SqlResult<()> {
        let mut connection = self.connection.borrow_mut();
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM Translations WHERE source_id = ?", [self.id])?;
        for translation in translations {
            transaction.execute(
                "INSERT INTO Translations (source_id, key, original, translation, comment) VALUES (?, ?, ?, ?, ?)",
                (self.id, translation.key, translation.original, translation.translation, translation.comment),
            )?;
        }
        transaction.commit()
    }

    /// Deletes this source.
    pub fn delete(self) -> SqlResult<()> {
        let mut connection = self.connection.borrow_mut();
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM Translations WHERE source_id = ?", [self.id])?;
        transaction.execute("DELETE FROM Sources WHERE id = ?", [self.id])?;
        transaction.commit()
    }
}
