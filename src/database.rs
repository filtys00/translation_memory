// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{cell::RefCell, collections::HashSet, str::FromStr};

use regex::Regex;
use reqwest::Url;
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
#[allow(clippy::type_complexity)]
const INIT_SQL: [(fn(&Connection) -> SqlResult<bool>, &str); 2] = [
    (|connection| connection.table_exists(None, "Providers"),
    r#"
    CREATE TABLE Languages (
        id INTEGER PRIMARY KEY,
        code TEXT NOT NULL UNIQUE
    ) STRICT;

    CREATE TABLE Providers (
        id INTEGER PRIMARY KEY,
        type                  TEXT NOT NULL CHECK (type IN ("builtin", "retired", "from_file")),
        code                  TEXT NOT NULL UNIQUE,
        name                  TEXT NOT NULL,
        group_name            TEXT,
        sources_download_time INTEGER,
        sources_has_failed    INTEGER NOT NULL DEFAULT 0
    ) STRICT;

    CREATE TABLE Sources (
        id INTEGER PRIMARY KEY,

        provider_id INTEGER NOT NULL,
        language_id INTEGER NOT NULL,

        originals_url        TEXT,
        translations_url     TEXT NOT NULL,

        download_time        INTEGER,
        originals_content    TEXT,
        translations_content TEXT,

        has_failed           INTEGER NOT NULL DEFAULT 0,

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
    "#),
    // Change the type of 'originals_content' and 'translations_content' to ANY.
    (|connection| {
        let translations_content_type: String = connection.query_one(
            "SELECT type FROM pragma_table_info('Sources') WHERE name = 'translations_content'", (),
            |row| row.get(0),
        )?;
        Ok(translations_content_type == "ANY")
    },
    r#"
    ALTER TABLE Sources RENAME COLUMN originals_content TO _originals_content;
    ALTER TABLE Sources ADD COLUMN originals_content ANY;
    UPDATE Sources SET originals_content = _originals_content;
    ALTER TABLE Sources DROP COLUMN _originals_content;

    ALTER TABLE Sources RENAME COLUMN translations_content TO _translations_content;
    ALTER TABLE Sources ADD COLUMN translations_content ANY;
    UPDATE Sources SET translations_content = _translations_content;
    ALTER TABLE Sources DROP COLUMN _translations_content;
    "#)
];

/// A connection to a translation database.
pub struct TranslationStore { connection: RefCell<Connection> }

/// A provider of translation sources.
pub struct Provider<'a> { connection: &'a RefCell<Connection>, id: i64 }

/// A source of translations.
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

pub struct ProviderNames {
    pub code: String,
    pub name: String,
    pub group_name: Option<String>,
}

pub struct SourceUrls {
    pub originals: Option<Url>,
    pub translations: Url,
}

#[derive(Clone)]
pub enum SourceContent {
    None,
    Text(String),
    Bytes(Vec<u8>),
}

impl SourceContent {
    pub fn is_none(&self) -> bool { matches!(self, Self::None) }
}

impl From<Option<String>> for SourceContent {
    fn from(value: Option<String>) -> Self {
        match value {
            None => Self::None,
            Some(value) => Self::Text(value),
        }
    }
}

impl FromSql for SourceContent {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str_or_null() {
            Ok(None) => Ok(Self::None),
            Ok(Some(value)) => Ok(Self::Text(value.to_string())),
            Err(_) => Ok(Self::Bytes(value.as_blob()?.to_vec()))
        }
    }
}

impl ToSql for SourceContent {
    fn to_sql(&self) -> SqlResult<ToSqlOutput<'_>> {
        match self {
            Self::None => Ok(ToSqlOutput::Borrowed(ValueRef::Null)),
            Self::Text(value) => Ok(ToSqlOutput::Borrowed(ValueRef::Text(value.as_bytes()))),
            Self::Bytes(value) => Ok(ToSqlOutput::Borrowed(ValueRef::Blob(value))),
        }
    }
}

pub struct SourceContents {
    pub originals: SourceContent,
    pub translations: SourceContent,
}

pub enum SourceFailed {
    None,
    Download,
    Parse,
}

impl SourceFailed {
    pub fn is_none(&self)     -> bool { matches!(self, Self::None)     }
    pub fn is_download(&self) -> bool { matches!(self, Self::Download) }
    pub fn is_parse(&self)    -> bool { matches!(self, Self::Parse)    }
    pub fn is_some(&self)     -> bool { !matches!(self, Self::None)    }
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
        for (has_init, init_sql) in INIT_SQL {
            if has_init(&connection)? { continue; }
            connection.execute_batch(init_sql)?;
        }

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
    /// Returns whether this provider has failed at downloading sources.
    pub fn has_sources_failed(&self) -> SqlResult<bool> {
        let failed = self.connection.borrow().query_one(
            "SELECT sources_has_failed FROM Providers WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(failed)
    }

    /// Set that this provider has failed at downloading sources.
    pub fn set_sources_failed(&self) -> SqlResult<()> {
        self.connection.borrow()
            .execute("UPDATE Providers SET sources_has_failed = true WHERE id = ?", [self.id])?;
        Ok(())
    }

    /// Returns a set of all the languages that have at least one source.
    pub fn get_source_languages(&self) -> SqlResult<HashSet<LanguageIdentifier>> {
        let languages = self.connection.borrow()
            .prepare("SELECT UNIQUE Languages.code FROM Sources JOIN Languages ON Sources.language_id = Languages.id WHERE Sources.provider_id = ?")?
            .query_map([self.id], |row| {
                LanguageIdentifier::from_str(row.get_ref(0)?.as_str()?).map_err(|e| {
                    SqlError::FromSqlConversionFailure(0, SqlType::Text, Box::new(e))
                })
            })?
            .collect::<SqlResult<_>>()?;
        Ok(languages)
    }

    /// Returns a list of all the sources that belongs to this provider.
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

    /// Returns the database ID of `lang_id`, adding it to the database if it does not exist.
    fn get_or_add_language_id(&self, lang_id: &LanguageIdentifier) -> SqlResult<i64> {
        let connection = self.connection.borrow();
        let language_id: Option<i64> = connection
            .query_one(
                "SELECT id FROM Languages WHERE code = ?", [lang_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(language_id) = language_id {
            Ok(language_id)
        } else {
            connection.execute("INSERT INTO Languages (code) VALUES (?)", [lang_id.to_string()])?;
            Ok(connection.last_insert_rowid())
        }
    }

    /// Set all the sources with language `lang_id` that belongs to this provider to `urls`.
    pub fn set_sources(&'_ self, lang_id: &LanguageIdentifier, urls: &[SourceUrls]) -> SqlResult<()> {
        let language_id = self.get_or_add_language_id(lang_id)?;
        let mut connection = self.connection.borrow_mut();
        let transaction = connection.transaction()?;
        transaction.execute("DELETE Sources WHERE provider_id = ?, language_id = ?", (self.id, language_id))?;
        for urls in urls {
            transaction.execute(
                "INSERT INTO Sources (provider_id, language_id, originals_url, translations_url) VALUES (?, ?, ?, ?)",
                (self.id, language_id, &urls.originals, &urls.translations),
            )?;
        }
        transaction.commit()
    }
}

impl Source<'_> {
    /// Returns the language of this source.
    pub fn get_language(&self) -> SqlResult<LanguageIdentifier> {
        let lang_id = self.connection.borrow().query_one(
            "SELECT Languages.code FROM Sources JOIN Languages ON Sources.language_id = Language.id WHERE Sources.id = ?", [self.id],
            |row| {
                LanguageIdentifier::from_str(row.get_ref(0)?.as_str()?).map_err(|e| {
                    SqlError::FromSqlConversionFailure(0, SqlType::Text, Box::new(e))
                })
            },
        )?;
        Ok(lang_id)
    }

    /// Returns the urls of this source.
    pub fn get_urls(&self) -> SqlResult<SourceUrls> {
        let urls = self.connection.borrow().query_one(
            "SELECT originals_url, translations_url FROM Sources WHERE id = ?", [self.id],
            |row| Ok(SourceUrls { originals: row.get(0)?, translations: row.get(1)? }),
        )?;
        Ok(urls)
    }

    /// Returns the download time of this source in unix time.
    pub fn get_download_time(&self) -> SqlResult<Option<u32>> {
        let download_time = self.connection.borrow().query_one(
            "SELECT download_time FROM Sources WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(download_time)
    }

    /// Returns the downloaded text content of this source.
    pub fn get_contents(&self) -> SqlResult<SourceContents> {
        let texts = self.connection.borrow().query_one(
            "SELECT originals_content, translations_content FROM Sources WHERE id = ?", [self.id],
            |row| {
                let texts = SourceContents {
                    originals: row.get(0)?,
                    translations: row.get(1)?,
                };
                Ok(texts)
            }
        )?;
        Ok(texts)
    }

    /// Set the downloaded text content of this source, using the current time as download time.
    pub fn set_contents(&self, contents: SourceContents) -> SqlResult<()> {
        self.connection.borrow().execute(
            "UPDATE Sources SET download_time = unixepoch(), originals_content = ?, translations_content = ?, failed = 0 WHERE id = ?",
            (contents.originals, contents.translations, self.id),
        )?;
        Ok(())
    }

    /// Returns whether this source has failed.
    pub fn has_failed(&self) -> SqlResult<SourceFailed> {
        let (download_time, failed): (Option<u32>, bool) = self.connection.borrow().query_one(
            "SELECT download_time, has_failed FROM Sources WHERE id = ?", [self.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if !failed {
            Ok(SourceFailed::None)
        } else if download_time.is_none() {
            Ok(SourceFailed::Download)
        } else {
            Ok(SourceFailed::Parse)
        }
    }

    /// Set whether this source has failed.
    pub fn set_failed(&self) -> SqlResult<()> {
        self.connection.borrow()
            .execute("UPDATE Sources SET has_failed = true WHERE id = ?", [self.id])?;
        Ok(())
    }

    /// Set the translations that belong to this source.
    pub fn set_translations(&self, translations: &[Translation]) -> SqlResult<()> {
        let mut connection = self.connection.borrow_mut();
        let transaction = connection.transaction()?;

        transaction.execute("DELETE FROM Translations WHERE source_id = ?", [self.id])?;
        let mut stmt = transaction.prepare(
            "INSERT INTO Translations (source_id, key, original, translation, comment) VALUES (?, ?, ?, ?, ?)"
        )?;
        for translation in translations {
            stmt.execute((
                self.id,
                &translation.key,
                &translation.original,
                &translation.translation,
                &translation.comment,
            ))?;
        }
        stmt.finalize()?;
        transaction.execute("UPDATE Sources SET has_failed = 0 WHERE source_id = ?", [self.id])?;

        transaction.commit()
    }
}
