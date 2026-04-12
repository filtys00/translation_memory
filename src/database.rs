// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{cell::RefCell, collections::HashSet, fmt::Debug, str::FromStr};

use log::trace;
use regex::Regex;
use reqwest::Url;
use rusqlite::{
    functions::FunctionFlags,
    types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, Type as SqlType, ValueRef},
    Connection,
    OptionalExtension,
    Error as SqlError,
    Params,
    Result as SqlResult,
};
use unic_langid::LanguageIdentifier;

/// SQL to initialize the SQLite database.
#[allow(clippy::type_complexity)]
const INIT_SQL: &[(fn(&Connection) -> SqlResult<bool>, &str)] = &[
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
    "#),
    // Add a 'has_parsed' field to source
    (|connection| connection.column_exists(None, "Sources", "has_parsed"),
    r#"
    ALTER TABLE Sources ADD COLUMN has_parsed INTEGER NOT NULL DEFAULT 0;
    "#),
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

#[allow(dead_code)]
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

    /// Delete a language along with all associated sources and translations.
    pub fn delete_language(&self, lang_id: &LanguageIdentifier) -> SqlResult<()> {
        let mut connection = self.connection.borrow_mut();
        let transaction = connection.transaction()?;
        let language_id: i32 = transaction.query_one(
            "SELECT id FROM Languages WHERE code = ?", [lang_id.to_string()],
            |row| row.get(0),
        )?;
        transaction.execute("DELETE FROM Translations WHERE source_id IN (SELECT id FROM Sources WHERE language_id = ?)", [language_id])?;
        transaction.execute("DELETE FROM Sources WHERE language_id = ?", [language_id])?;
        transaction.execute("DELETE FROM Languages WHERE id = ?", [language_id])?;
        transaction.commit()
    }

    /// Get a count of all the sources that are of the language `lang_id`.
    pub fn count_sources_by_lang(&self, lang_id: &LanguageIdentifier) -> SqlResult<u32> {
        let count = self.connection.borrow().query_one(
            "SELECT count(*) FROM Sources
            JOIN Languages ON Languages.id = Sources.language_id
            WHERE Languages.code = ?",
            [lang_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get a count of all the translations that are of the language `lang_id`.
    pub fn count_translations_by_lang(&self, lang_id: &LanguageIdentifier) -> SqlResult<u32> {
        let count = self.connection.borrow().query_one(
            "SELECT count(*) FROM Translations
            JOIN Sources ON Sources.id = Translations.source_id
            JOIN Languages ON Languages.id = Sources.language_id
            WHERE Languages.code = ?",
            [lang_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(count)
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

    /// Add a new provider.
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
        let code = self.connection.borrow().query_one(
            "SELECT type FROM Providers WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(code)
    }

    /// Set the type of this provider.
    pub fn set_type(&self, provider_type: ProviderType) -> SqlResult<()> {
        self.connection.borrow()
            .execute("UPDATE Providers SET type = ? WHERE id = ?", (provider_type, self.id))?;
        Ok(())
    }

    /// Returns the code name of this provider.
    pub fn get_code(&self) -> SqlResult<String> {
        let code = self.connection.borrow().query_one(
            "SELECT code FROM Providers WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(code)
    }

    /// Returns the code name, name and group name of this provider.
    pub fn get_names(&self) -> SqlResult<ProviderNames> {
        let code = self.connection.borrow().query_one(
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
        Ok(code)
    }

    /// Set the name and group name of this provider.
    pub fn set_names(&self, name: &str, group_name: Option<&str>) -> SqlResult<()> {
        self.connection.borrow().execute(
            "UPDATE Providers SET name = ?, group_name = ? WHERE id = ?",
            (name, group_name, self.id)
        )?;
        Ok(())
    }

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
            .execute("UPDATE Providers SET sources_has_failed = 1 WHERE id = ?", [self.id])?;
        Ok(())
    }

    /// Returns a set of all the languages that have at least one source.
    pub fn get_source_languages(&self) -> SqlResult<HashSet<LanguageIdentifier>> {
        let languages = self.connection.borrow()
            .prepare("SELECT DISTINCT Languages.code FROM Sources JOIN Languages ON Sources.language_id = Languages.id WHERE Sources.provider_id = ?")?
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
                let source = Source { connection: self.connection, id: row.get(0)? };
                Ok(source)
            })?
            .collect::<SqlResult<_>>()?;
        Ok(sources)
    }

    /// Returns a list of all the sources that belongs to this provider and have `lang_id`.
    pub fn get_sources_with_language(&self, lang_id: &LanguageIdentifier) -> SqlResult<Vec<Source<'_>>> {
        let sources = self.connection.borrow()
            .prepare("SELECT Sources.id FROM Sources JOIN Languages ON Sources.language_id = Languages.id WHERE Languages.code = ?")?
            .query_map([lang_id.to_string()], |row| {
                let source = Source { connection: self.connection, id: row.get(0)? };
                Ok(source)
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

    /// Replace all the sources, with language `lang_id`, that belongs to this provider with `urls`.
    pub fn set_sources(&'_ self, lang_id: &LanguageIdentifier, urls: &[SourceUrls]) -> SqlResult<()> {
        let language_id = self.get_or_add_language_id(lang_id)?;
        let mut connection = self.connection.borrow_mut();
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE Providers SET sources_has_failed = 0 WHERE id = ?", [self.id])?;
        transaction.execute("DELETE FROM Translations WHERE source_id IN (SELECT id FROM Sources WHERE provider_id = ? AND language_id = ?)", (self.id, language_id))?;
        transaction.execute("DELETE FROM Sources WHERE provider_id = ? AND language_id = ?", (self.id, language_id))?;
        for urls in urls {
            transaction.execute(
                "INSERT INTO Sources (provider_id, language_id, originals_url, translations_url) VALUES (?, ?, ?, ?)",
                (self.id, language_id, &urls.originals, &urls.translations),
            )?;
        }
        transaction.commit()
    }

    /// Replace all the sources, with language `lang_id`, that belongs to this provider with `urls`.
    pub fn set_source(&'_ self, lang_id: &LanguageIdentifier, urls: SourceUrls) -> SqlResult<Source<'_>> {
        self.set_sources(lang_id, &[urls])?;
        Ok(Source { connection: self.connection, id: self.connection.borrow().last_insert_rowid() })
    }

    /// Get a count of all the sources that are of the language `lang_id` that belong to this provider.
    pub fn count_sources(&self) -> SqlResult<u32> {
        let count = self.connection.borrow().query_one(
            "SELECT count(*) FROM Sources WHERE Sources.provider_id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get a count of all the translations that belong to this provider.
    pub fn count_translations(&'_ self) -> SqlResult<u32> {
        let count = self.connection.borrow().query_one(
            "SELECT count(*) FROM Translations
            JOIN Sources ON Sources.id = Translations.source_id
            WHERE provider_id = ?",
            [self.id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Delete this provider.
    pub fn delete(self) -> SqlResult<()> {
        let mut connection = self.connection.borrow_mut();
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM Translations WHERE source_id IN (SELECT id FROM Sources WHERE provider_id = ?)", [self.id])?;
        transaction.execute("DELETE FROM Sources WHERE provider_id = ?", [self.id])?;
        transaction.execute("DELETE FROM Providers WHERE id = ?", [self.id])?;
        transaction.commit()
    }
}

impl Source<'_> {
    /// Returns the language of this source.
    pub fn get_language(&self) -> SqlResult<LanguageIdentifier> {
        let lang_id = self.connection.borrow().query_one(
            "SELECT Languages.code FROM Sources JOIN Languages ON Sources.language_id = Languages.id WHERE Sources.id = ?", [self.id],
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
            "UPDATE Sources SET download_time = unixepoch(), originals_content = ?, translations_content = ?, has_failed = 0 WHERE id = ?",
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
        transaction.execute("UPDATE Sources SET has_failed = 0, has_parsed = 1 WHERE id = ?", [self.id])?;

        transaction.commit()
    }

    /// Returns whether this source has been parsed.
    pub fn has_parsed(&self) -> SqlResult<bool> {
        let has_parsed = self.connection.borrow().query_one(
            "SELECT has_parsed FROM Sources WHERE id = ?", [self.id],
            |row| row.get(0),
        )?;
        Ok(has_parsed)
    }

    /// Delete this source.
    pub fn delete(self) -> SqlResult<()> {
        let mut connection = self.connection.borrow_mut();
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM Translations WHERE source_id = ?", [self.id])?;
        transaction.execute("DELETE FROM Sources WHERE id = ?", [self.id])?;
        transaction.commit()
    }
}

// Query API

#[derive(Debug, Clone)]
pub enum QueryFilter {
    /// Applies if either the original or translation string matches `regex`.
    All { regex: Regex },
    /// Applies if the original string matches `regex`.
    Original { regex: Regex },
    /// Applies if the translation string matches `regex`.
    Translation { regex: Regex },
    /// Applies if the provider name, code name or group name is `name`.
    Provider { name: String },
    /// Applies if the provider name, code name or group name is one of `names`.
    Providers { names: Vec<String> },
    /// Applies if the translated to language is `lang_id`.
    Language { lang_id: String },
    /// Applies if the translated to language is one of `lang_ids`.
    Languages { lang_ids: Vec<String> },
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum QueryFilterMode { Require, Deny }

pub struct QueryOptions {
    pub limit: u32,
    pub offset: u32,
    pub filters: Vec<(QueryFilter, QueryFilterMode)>,
}

pub struct QueryCountOptions {
    pub filters: Vec<(QueryFilter, QueryFilterMode)>,
}

pub struct ExtendedTranslation {
    pub translations_url: String,
    pub provider_code: String,
    pub language_id: String,
    pub key: Option<String>,
    pub original: String,
    pub translation: String,
    pub comment: Option<String>,
}

/// Returns `filters` as SQL WHERE conditions and parameters.
fn filters_to_sql(filters: &[(QueryFilter, QueryFilterMode)]) -> (String, impl Params + Debug) {
    let mut where_conditions = String::from("1=1"); // Needs at least one condition if `filters` is empty
    let mut params: Vec<&str> = Vec::new();

    for (filter, mode) in filters {
        where_conditions += " AND "; // `where_conditions` has one default condition
        if *mode == QueryFilterMode::Deny { where_conditions += "NOT " }
        match filter {
            QueryFilter::All { regex } => {
                where_conditions += "(original REGEXP ? OR translation REGEXP ?)";
                params.push(regex.as_str());
                params.push(regex.as_str());
            },
            QueryFilter::Original { regex } => {
                where_conditions += "original REGEXP ?";
                params.push(regex.as_str());
            },
            QueryFilter::Translation { regex } => {
                where_conditions += "translation REGEXP ?";
                params.push(regex.as_str());
            },
            QueryFilter::Provider { name } => {
                where_conditions += "(Providers.code = ? OR Providers.name = ? OR Providers.group_name IS ?)";
                params.push(name);
                params.push(name);
                params.push(name);
            },
            QueryFilter::Providers { names } => {
                where_conditions += "(1=0"; // Default is false, if `names` is empty
                for name in names {
                    where_conditions += " OR Providers.code = ? OR Providers.name = ? OR Providers.group_name IS ?";
                    params.push(name);
                    params.push(name);
                    params.push(name);
                }
                where_conditions += ")";
            },
            QueryFilter::Language { lang_id } => {
                where_conditions += "Languages.code = ?";
                params.push(lang_id);
            },
            QueryFilter::Languages { lang_ids } => {
                where_conditions += "(1=0"; // Default is false, if `lang_ids` is empty
                for lang_id in lang_ids {
                    where_conditions += " OR Languages.code = ?";
                    params.push(lang_id);
                }
                where_conditions += ")";
            },
        }
    }

    (where_conditions, rusqlite::params_from_iter(params))
}

impl TranslationStore {
    /// Query translations according to `options`.
    pub fn query_translations(&self, options: QueryOptions) -> anyhow::Result<Vec<ExtendedTranslation>> {
        let (where_conditions, params) = filters_to_sql(&options.filters);

        let sql = format!("
            SELECT Sources.translations_url, Providers.code, Languages.code, key, original, translation, comment
            FROM Translations
            JOIN Sources ON Translations.source_id = Sources.id
            JOIN Providers ON Sources.provider_id = Providers.id
            JOIN Languages ON Sources.language_id = Languages.id
            WHERE {where_conditions} LIMIT {} OFFSET {}
        ", options.limit, options.offset); // Inline numeric args to avoid fighting the type system
        trace!("SQL Query: {sql}\nParameters: {params:?}");

        let connection = self.connection.borrow();
        let translations = connection
            .prepare(&sql)?
            .query_map(params, |row| -> SqlResult<ExtendedTranslation> {
                let translation = ExtendedTranslation {
                    translations_url: row.get(0)?,
                    provider_code: row.get(1)?,
                    language_id: row.get(2)?,
                    key: row.get(3)?,
                    original: row.get(4)?,
                    translation: row.get(5)?,
                    comment: row.get(6)?,
                };
                Ok(translation)
            })?
            .collect::<SqlResult<_>>()?;

        Ok(translations)
    }

    // Make the count and translation queryies separate
    // so the translations query do not need to wait for the count query.

    /// Query translations according to `options`, and return the total amount of translations.
    pub fn query_translation_count(&self, options: QueryCountOptions) -> anyhow::Result<u32> {
        let (where_conditions, params) = filters_to_sql(&options.filters);

        let connection = self.connection.borrow();
        let total_count = connection.query_one(
            &format!("
                SELECT COUNT(*) FROM Translations
                JOIN Sources ON Translations.source_id = Sources.id
                JOIN Providers ON Sources.provider_id = Providers.id
                JOIN Languages ON Sources.language_id = Languages.id
                WHERE {where_conditions}
            "),
            params,
            |row| row.get(0),
        )?;
        Ok(total_count)
    }
}
