// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{cmp::Ordering, collections::{HashMap, HashSet}, fs, io::{self, Write}, path::PathBuf, sync::{Arc, LazyLock}};

use anyhow::{anyhow, bail};
use clap::{Subcommand, ValueEnum, builder::PossibleValue};
use log::{error, info, warn};
use reqwest::Url;
use termcolor::StandardStream;
use tokio::{io::{AsyncReadExt, stdin}, select, runtime::Runtime as TokioRuntime, sync::Mutex};
use unic_langid::LanguageIdentifier;

use crate::{
    database::{ProviderNames, ProviderType, SourceContent, SourceContents, SourceUrls, Translation, TranslationStore},
    providers::{builtin, Downloader, RetryPolicy},
    web_server::web_server,
};

#[derive(Debug, Clone, ValueEnum)]
pub enum RetryValue {
    None,
    Parse,
    Download,
    Failed,
    All,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum ParserValue {
    AndroidXml,
    BrowserExtension,
    MicrosoftTbx,
    Po,
    Properties,
}

#[allow(clippy::type_complexity)]
enum Parser {
    Mono(fn(String) -> anyhow::Result<Vec<Translation>>),
    Duo(fn(String) -> anyhow::Result<HashMap<String, (String, Option<String>)>>),
}
impl ParserValue {
    /// Returns the function for parsing translations.
    fn get_parser(&self) -> Parser {
        match self {
            ParserValue::AndroidXml => Parser::Duo(builtin::parse_android),
            ParserValue::BrowserExtension => Parser::Duo(builtin::parse_browser_extension),
            ParserValue::MicrosoftTbx => Parser::Mono(builtin::parse_microsoft_tbx),
            ParserValue::Po => Parser::Mono(builtin::parse_po),
            ParserValue::Properties => Parser::Duo(builtin::parse_properties),
        }
    }
}

/// List of parser values that are duo parsers.
static DUO_PARSER_VALUES: LazyLock<Vec<PossibleValue>> = LazyLock::new(||
    ParserValue::value_variants().iter()
        .filter(|v| matches!(v.get_parser(), Parser::Duo { .. }))
        .filter_map(|v| v.to_possible_value())
        .collect()
);

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start a web UI server for querying translations.
    Start {
        /// Open the web UI in the default web browser.
        #[arg(long = "openbrowser")]
        open_browser: bool,
    },
    /// Download translations for one or more providers.
    #[command(alias = "dl")]
    Download {
        #[arg()]
        /// Providers to download for.
        provider_codes: Vec<String>,

        /// Languages to download for.
        #[arg(short, long = "languages", value_delimiter = ',')]
        lang_ids: Vec<LanguageIdentifier>,

        /// Weather to retry finished and failed downloads.
        #[arg(short, long)]
        retry: Option<RetryValue>,
    },
    /// Add a new provider with translations.
    Add {
        #[arg()]
        /// The code name of the provider.
        code: String,

        #[arg(short, long)]
        /// The name of the provider.
        name: Option<String>,

        #[arg(short, long, requires = "name")]
        /// The group name of the provider.
        group_name: Option<String>,

        #[arg(short, long = "language")]
        /// The language of the translations.
        lang_id: LanguageIdentifier,

        #[arg(short, long)]
        /// The file format of the translations file.
        parser: ParserValue,

        #[arg(short, long, required_if_eq_any(DUO_PARSER_VALUES.iter().map(|v| ("parser", v.get_name()))))]
        /// Path to the file containing the default strings if the parser requires it.
        originals_file: Option<PathBuf>,
        
        #[arg(short, long)]
        /// Path to the file containing the translations.
        translations_file: PathBuf,
    },
    /// Remove the translations of one or more providers.
    #[command(alias = "rm")]
    Remove {
        #[arg()]
        /// Providers to remove.
        provider_codes: Vec<String>,

        /// Only remove the translations for certain languages.
        #[arg(short, long = "languages", value_delimiter = ',')]
        lang_ids: Vec<LanguageIdentifier>,
    },
    /// Print translation and provider status information.
    Status {
        /// Print all providers without hiding any.
        #[arg(short, long)]
        all: bool,
    },
}

/// Perform the action associated with `command`.
pub fn perform_command(
    command: Command,
    mut console: StandardStream,
    term_width: usize,
    db: TranslationStore,
) -> anyhow::Result<()> {
    match command {
        Command::Start { open_browser } => {
            if open_browser {
                info!("Opening web browser...");
                webbrowser::open("http://127.0.0.1:2013/")?;
            }

            info!("Starting web UI server at 'http://127.0.0.1:2013/'...");
            info!("Press 'q' or 'Ctrl+C' to stop the server");
            let stop_listener = async {
                crossterm::terminal::enable_raw_mode()?;
                loop {
                    let c = stdin().read_u8().await?;
                    if c == b'q' || c == 3 /* Ctrl+C */ {
                        break;
                    }
                    warn!("Press 'q' or 'Ctrl+C' to stop the server");
                }
                crossterm::terminal::disable_raw_mode()?;
                Ok::<_, anyhow::Error>(())
            };
            let store = Arc::new(Mutex::new(db));
            let runtime = TokioRuntime::new()?;
            runtime.block_on(async {
                select! {
                    result = web_server(store) => {
                        result.map_err(|e| anyhow!("Could not start web UI server: {e}"))?
                    }
                    result = stop_listener => { result? }
                }
                Ok::<(), anyhow::Error>(())
            })?;

            Ok(())
        },
        Command::Download { provider_codes, lang_ids, retry } => {
            let mut providers = builtin::providers();
            if !provider_codes.is_empty() {
                for arg_code in &provider_codes {
                    let is_match = providers.iter().any(|provider| {
                        if arg_code.ends_with('-') {
                            provider.code().starts_with(arg_code)
                        } else {
                            provider.code() == arg_code
                        }
                    });
                    if !is_match { bail!("No provider with code '{arg_code}'"); }
                }
                providers.retain(|provider| {
                    provider_codes.iter().any(|arg_code| {
                        if arg_code.ends_with('-') {
                            provider.code().starts_with(arg_code)
                        } else {
                            provider.code() == arg_code
                        }
                    })
                });
            }

            let lang_ids: HashSet<_> = if lang_ids.is_empty() {
                db.get_languages()?
            } else {
                lang_ids.into_iter().collect()
            };
            if lang_ids.is_empty() { bail!("No languages are specified"); }


            let retry_policy = match retry {
                None if !provider_codes.is_empty() => RetryPolicy {
                    download_failed_sources: true,
                    download_failed_source: true,
                    parse_failed_source: true,
                    ..RetryPolicy::default()
                },
                None | Some(RetryValue::None) => RetryPolicy::default(),
                Some(RetryValue::Parse) => RetryPolicy {
                    parse_failed_source: true,
                    parse_finished_source: true,
                    ..RetryPolicy::default()
                },
                Some(RetryValue::Download) => RetryPolicy {
                    download_finished_sources: true,
                    download_failed_sources: true,
                    download_finished_source: true,
                    download_failed_source: true,
                    ..RetryPolicy::default()
                },
                Some(RetryValue::Failed) => RetryPolicy {
                    download_failed_sources: true,
                    download_failed_source: true,
                    parse_failed_source: true,
                    ..RetryPolicy::default()
                },
                Some(RetryValue::All) => RetryPolicy {
                    download_finished_sources: true,
                    download_failed_sources: true,
                    download_finished_source: true,
                    download_failed_source: true,
                    parse_finished_source: true,
                    parse_failed_source: true,
                },
            };

            let downloader = Downloader::new()?;

            for (i, provider) in providers.iter().enumerate() {
                info!("Downloading provider '{}' ({}/{})", provider.code(), i + 1, providers.len());

                let db_provider = if let Some(db_provider) = db.get_provider(provider.code())? {
                    db_provider
                } else {
                    db.add_provider(ProviderType::BuiltIn, ProviderNames {
                        code: provider.code().to_string(),
                        name: provider.name().to_string(),
                        group_name: provider.group_name().map(|n| n.to_string()),
                    })?
                };
                match db_provider.get_type()? {
                    ProviderType::BuiltIn => {},
                    ProviderType::Retired => { db_provider.set_type(ProviderType::BuiltIn)?; },
                    ProviderType::FromFile => {
                        bail!("Provider '{}' has already been added from file", provider.code())
                    },
                }
                let names = db_provider.get_names()?;
                if names.name != provider.name() || names.group_name.as_deref() != provider.group_name() {
                    db_provider.set_names(provider.name(), provider.group_name())?;
                }

                if let Err(e) = provider.download(&lang_ids, &db_provider, &downloader, &retry_policy) {
                    error!("Could not download translations for provider '{}': {e}", provider.code());
                }
            }

            if provider_codes.is_empty() {
                for db_provider in db.get_providers()? {
                    let db_code = db_provider.get_code()?;
                    let db_is_builtin = db_provider.get_type()? == ProviderType::BuiltIn;
                    let provider_with_code = providers.iter()
                        .any(|provider| provider.code() == db_code);
                    if db_is_builtin && !provider_with_code {
                        db_provider.set_type(ProviderType::Retired)?;
                    }

                }
            }

            Ok(())
        },
        Command::Add {
            code, name, group_name,
            lang_id, parser,
            originals_file, translations_file,
        } => {
            let translations_text = fs::read_to_string(&translations_file)?;
            let translations_content = SourceContent::Text(translations_text.clone());

            let (translations, originals_content) = match parser.get_parser() {
                Parser::Duo(parse) => {
                    let originals_text = if let Some(originals_file) = &originals_file {
                        fs::read_to_string(originals_file)?
                    } else {
                        unreachable!("Cannot parse with '{parser:?}' without an originals file");
                    };
                    let originals_content = SourceContent::Text(originals_text.clone());

                    let translations = builtin::merge_messages(
                        parse(translations_text)?,
                        parse(originals_text)?,
                    );
                    (translations, originals_content)
                },
                Parser::Mono(parse) => {
                    let translations = parse(translations_text)?;
                    (translations, SourceContent::None)
                },
            };

            let provider = if let Some(provider) = db.get_provider(&code)? {
                if provider.get_type()? != ProviderType::FromFile {
                    bail!("Cannot add to builtin provider '{code}'");
                }
                if let Some(name) = name {
                    provider.set_names(&name, group_name.as_deref())?;
                };
                provider
            } else {
                let Some(name) = name else { bail!("Cannot add new provider without a name"); };
                db.add_provider(ProviderType::FromFile, ProviderNames { code, name, group_name })?
            };

            let originals_url = if let Some(originals_file) = originals_file {
                let url = Url::from_file_path(originals_file)
                    .map_err(|_| anyhow!("Could not resolve originals path to an URL"))?;
                Some(url)
            } else {
                None
            };
            let translations_url = Url::from_file_path(translations_file)
                .map_err(|_| anyhow!("Could not resolve translations path to an URL"))?;
            let source = provider.set_source(&lang_id, SourceUrls {
                originals: originals_url,
                translations: translations_url,
            })?;

            source.set_contents(SourceContents {
                originals: originals_content,
                translations: translations_content,
            })?;
            source.set_translations(&translations)?;

            Ok(())
        },
        Command::Remove { provider_codes, lang_ids } => {
            for code in &provider_codes {
                let Some(db_provider) = db.get_provider(code)? else {
                    bail!("No provider with code '{code}'");
                };
                for lang_id in &lang_ids {
                    for db_source in db_provider.get_sources_with_language(lang_id)? {
                        db_source.delete()?;
                    }
                }
                if lang_ids.is_empty() { db_provider.delete()?; }
            }
            if provider_codes.is_empty() {
                for lang_id in lang_ids {
                    db.delete_language(&lang_id)?;
                }
            }

            Ok(())
        },
        Command::Status { all } => {
            // Divide providers into groups
            let mut builtin_providers: HashSet<_> = builtin::providers().into_iter()
                .map(|provider| provider.code().to_string())
                .collect();
            let mut finished_providers = Vec::new();
            let mut unfinished_providers = Vec::new();
            let mut failed_providers = Vec::new();
            for provider in db.get_providers()? {
                let code = provider.get_code()?;
                builtin_providers.remove(&code);

                if provider.has_sources_failed()? {
                    failed_providers.push((code, (0, 0)));
                    continue;
                }

                let mut finished: u32 = 0;
                let mut unfinished: u32 = 0;
                let mut failed: u32 = 0;
                for source in provider.get_sources()? {
                    if source.has_failed()?.is_some() {
                        failed += 1;
                    } else if source.get_download_time()?.is_some() {
                        finished += 1;
                    } else {
                        unfinished += 1;
                    }
                }

                let total = finished + unfinished + failed;
                if finished > 0   {   finished_providers.push((code.clone(), (finished,   total))); }
                if unfinished > 0 { unfinished_providers.push((code.clone(), (unfinished, total))); }
                if failed > 0     {     failed_providers.push((code.clone(), (failed,     total))); }
            }
            for provider in builtin_providers {
                unfinished_providers.push((provider, (0, 0)));
            }

            // Print providers to console

            /// Format providers to be displayed.
            fn format_providers(
                mut all_providers: Vec<(String, (u32, u32))>,
                all: bool,
                partial_first: bool,
                partial_color: &str,
                full_color: &str,
            ) -> Vec<String> {
                // Sort providers
                all_providers.sort_by(|a, b| {
                    let av = a.1.0 == a.1.1;
                    let bv = b.1.0 == b.1.1;
                    if !partial_first &&  bv && !av { return Ordering::Greater }
                    if !partial_first && !bv &&  av { return Ordering::Less }
                    if  partial_first &&  bv && !av { return Ordering::Less }
                    if  partial_first && !bv &&  av { return Ordering::Greater }

                    let ord = (a.1.0 as f32 / a.1.1 as f32)
                        .partial_cmp(&(b.1.0 as f32 / b.1.1 as f32));
                    if ord == Some(Ordering::Less) { return Ordering::Less }
                    if ord == Some(Ordering::Greater) { return Ordering::Greater }

                    a.0.cmp(&b.0)
                });

                // Select what providers to display
                let mut providers: Vec<_> = all_providers.iter()
                    .filter(|(_, (amount, total))| amount != total || all)
                    .collect();
                if providers.is_empty() { providers = all_providers.iter().take(10).collect(); }

                // Format providers
                let mut codes: Vec<String> = providers.iter()
                    .map(|(code, (amount, total))| {
                        if amount == total {
                            format!("\x1b[{full_color}m{code}\x1b[0m")
                        } else {
                            format!("\x1b[{partial_color}m{code}\u{00A0}({amount}/{total})\x1b[0m")
                        }
                    })
                    .collect();
                if !all && all_providers.len() != codes.len() {
                    codes.push(format!("...and\u{00A0}{}\u{00A0}other(s)", all_providers.len() - codes.len()));
                }
                codes
            }

            let mut languages: Vec<String> = db.get_languages()?.into_iter()
                .map(|l| l.to_string())
                .collect();
            languages.sort();

            let titles = [
                ("Languages", languages),
                ("Finished providers",   format_providers(finished_providers, all, true, "1;33", "32")),
                ("Unfinished providers", format_providers(unfinished_providers, all, false, "1;33", "0")),
                ("Failed providers",     format_providers(failed_providers, true, false, "1;33", "31")),
            ];
            for (i, (title, codes)) in titles.iter().enumerate() {
                write!(console, "\x1b[1m{title}:\x1b[0m")?;
                if codes.is_empty() {
                    writeln!(console, " \x1b[3mnone\x1b[0m")?;
                } else {
                    write!(console, "\n  ")?;
                    writeln_max_width(
                        &mut console,
                        &codes.join("  "),
                        0, 2, term_width,
                    )?;
                }
                if i != titles.len() - 1 { writeln!(console)?; }
            }

            Ok(())
        }
    }
}

/// Write `args` to `buf`, preventing any line from becoming longer than `max_width`
/// and indenting every new line with `indent` amount of spaces.
///
/// `line_length` is the length of the current line.
///
/// # Panics
///
/// Panics if `indent` is not smaller than `max_width`.
pub fn writeln_max_width(
    mut buf: impl Write,
    args: &str,
    mut line_length: usize,
    indent: usize,
    max_width: usize,
) -> io::Result<()> {
    if args.is_empty() {
        return writeln!(buf);
    }

    macro_rules! new_line {
        () => {
            writeln!(buf)?;
            for _ in 0..indent {
                write!(buf, " ")?;
            }
            #[allow(unused_assignments)]
            {
                line_length = indent;
            }
        };
    }

    for (i, line) in args.split('\n').enumerate() {
        if line.is_empty() {
            writeln!(buf)?;
            line_length = 0;
            continue;
        }

        if i > 0 {
            new_line!();
        }

        if line_length + 1 + args.len() <= max_width {
            write!(buf, " {line}")?;
            line_length = 0;
            continue;
        }

        for term in line.split(' ') {
            if line_length + 1 + term.len() > max_width {
                if term.len() + 1 + indent > max_width {
                    let mut term = term;
                    while let Some(part) = term.get(..(max_width - line_length - 1)) {
                        term = &term[(max_width - line_length - 1)..];

                        write!(buf, " {part}")?;
                        new_line!();
                    }
                    write!(buf, " {term}")?;
                    line_length += 1 + term.len();
                    continue;
                } else {
                    new_line!();
                }
            }

            write!(buf, " {term}")?;
            line_length += 1 + term.len();
        }
    }

    writeln!(buf)
}
