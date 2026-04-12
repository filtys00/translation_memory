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
    providers::{Downloader, Progress, RetryPolicy, builtin},
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

        /// Print how many sources each provider have.
        #[arg(short = 's', long, conflicts_with = "show_translations")]
        show_sources: bool,

        /// Print how many translations each provider have.
        #[arg(short = 't', long, conflicts_with = "show_sources")]
        show_translations: bool,
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
                let on_progress = |progress| {
                    let reset = "\x1b[2K"; // Clear current line
                    let clear = "\x1b[0m";
                    let clear_highlight = "\x1b[0;1m";
                    let blue = "\x1b[36;1m";
                    let green = "\x1b[32;1m";
                    let gray = "\x1b[30m";

                    let ongoing_base = format_args!(
                        "{blue}Downloading {clear_highlight}{0}{clear}\t{1}/{2}",
                        provider.code(), i + 1, providers.len()
                    );

                    match progress {
                        Progress::DownloadingSources { lang_ids } => {
                            let languages = lang_ids.into_iter()
                                .map(|l| l.to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                            eprint!("{ongoing_base}, downloading sources for {languages}\r");
                        },
                        Progress::StartDownloadingSources { .. } => {
                            eprint!("{reset}{ongoing_base}\r");
                        },
                        Progress::DownloadingSource { current, total } => {
                            eprint!("{ongoing_base}, downloading source {current}/{total}\r");
                        },
                        Progress::StartParsingSources { .. } => {
                            eprint!("{reset}{ongoing_base}\r");
                        },
                        Progress::ParsingSource { current, total } => {
                            eprint!("{ongoing_base}, parsing source {current}/{total}\r");
                        },
                        Progress::Done { downloaded_sources, parsed_sources } => {
                            let base = format_args!(
                                "{reset}{green}Downloaded {clear_highlight}{0}{clear}",
                                provider.code(),
                            );

                            if downloaded_sources > 0 { // Always print parsed sources when at least one source were downloaded
                                println!("{base} {gray}(downloaded {downloaded_sources} sources, parsed {parsed_sources} sources){clear}");
                            } else if parsed_sources > 0 {
                                println!("{base} {gray}(parsed {parsed_sources} sources){clear}");
                            } else if !provider_codes.is_empty() { // Only print when nothing changed if explicit
                                println!("{base}");
                            }
                        },
                    };
                    // Must flush so that stderr() is not line-buffered, but printed immediately
                    io::stderr().flush()?;
                    Ok(())
                };

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

                if let Err(e) = provider.download(&lang_ids, &db_provider, &downloader, &retry_policy, on_progress) {
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
                    bail!("Cannot add to built-in provider '{code}'");
                }
                if let Some(name) = name {
                    provider.set_names(&name, group_name.as_deref())?;
                };
                provider
            } else {
                if builtin::providers().iter().any(|provider| provider.code() == code) {
                    bail!("Cannot add to built-in provider '{code}'");
                }
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
        Command::Status { all, show_sources, show_translations } => {
            #[derive(Clone)]
            enum Info {
                None,
                Number(u32),
                Fraction { amount: u32, total: u32 },
            }

            let mut languages = Vec::new();
            for lang_id in db.get_languages()? {
                let info = if show_sources {
                    Info::Number(db.count_sources_by_lang(&lang_id)?)
                } else if show_translations {
                    Info::Number(db.count_translations_by_lang(&lang_id)?)
                } else {
                    Info::None
                };
                languages.push((lang_id.to_string(), info));
            }

            let mut builtin_provider_codes: HashSet<_> = builtin::providers().into_iter()
                .map(|provider| provider.code().to_string())
                .collect();

            let mut builtin_providers = Vec::new();
            let mut retired_providers = Vec::new();
            let mut added_providers = Vec::new();

            let mut finished_providers = Vec::new();
            let mut unfinished_providers = Vec::new();
            let mut failed_providers = Vec::new();

            for provider in db.get_providers()? {
                let code = provider.get_code()?;
                builtin_provider_codes.remove(&code);

                let info = if show_sources {
                    Info::Number(provider.count_sources()?)
                } else if show_translations {
                    Info::Number(provider.count_translations()?)
                } else {
                    Info::None
                };

                match provider.get_type()? {
                    ProviderType::BuiltIn => { builtin_providers.push((code.clone(), info.clone())); },
                    ProviderType::Retired => { retired_providers.push((code.clone(), info.clone())); },
                    ProviderType::FromFile => { added_providers.push((code.clone(), info.clone())); },
                }

                // Skip work if will not be needed.
                if show_sources || show_translations { continue; }

                if provider.has_sources_failed()? {
                    failed_providers.push((code, info.clone()));
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
                let lists = [
                    (finished, &mut finished_providers),
                    (unfinished, &mut unfinished_providers),
                    (failed, &mut failed_providers),
                ];
                for (count, providers) in lists {
                    if count == 0 { continue; }
                    let info = if count == total { Info::None } else {
                        Info::Fraction { amount: count, total }
                    };
                    providers.push((code.clone(), info));
                }
            }

            for provider in builtin_provider_codes {
                unfinished_providers.push((provider, Info::None));
            }

            // Print providers to console

            /// Format providers to be displayed.
            fn format(mut codes: Vec<(String, Info)>, show_all: bool, color: Option<&str>) -> Vec<String> {
                codes.sort_by(|(a_code, a_info), (b_code, b_info)| {
                    match (a_info, b_info) {
                        (Info::None, Info::Number(_)) => Ordering::Less,
                        (Info::Number(_), Info::None) => Ordering::Greater,

                        (Info::None, Info::Fraction { .. }) => Ordering::Less,
                        (Info::Fraction { .. }, Info::None) => Ordering::Greater,

                        (Info::Number(_), Info::Fraction { .. }) => Ordering::Less,
                        (Info::Fraction { .. }, Info::Number(_)) => Ordering::Greater,

                        (Info::None, Info::None) => a_code.cmp(b_code),
                        (Info::Number(a_num), Info::Number(b_num)) => {
                            a_num.cmp(b_num).reverse().then_with(|| a_code.cmp(b_code))
                        },
                        (Info::Fraction { amount: a_amount, total: a_total }, Info::Fraction { amount: b_amount, total: b_total }) => {
                            (*a_amount as f32 / *a_total as f32)
                                .partial_cmp(&(*b_amount as f32 / *b_total as f32))
                                .unwrap_or(Ordering::Equal)
                                .reverse()
                                .then_with(|| a_code.cmp(b_code))
                        }
                    }
                });

                // Select which providers to display
                let mut shown_codes: Vec<_> = codes.iter()
                    .filter(|(_, info)| show_all || !matches!(info, Info::None))
                    .take(if show_all { usize::MAX } else { 40 })
                    .collect();
                if shown_codes.is_empty() { shown_codes = codes.iter().take(10).collect(); }
                if codes.len() - shown_codes.len() < 3 { shown_codes = codes.iter().collect(); }

                // Format providers
                let mut shown_codes: Vec<String> = shown_codes.iter()
                    .map(|(code, info)| match info {
                        Info::None if color.is_none() => code.clone(),
                        Info::Number(num) if color.is_none() => format!("{code}\u{00A0}({num})"),
                        Info::None => {
                            format!("\x1b[{}m{code}\x1b[0m", color.unwrap_or("0"))
                        },
                        Info::Number(num) => {
                            format!("\x1b[{}m{code}\u{00A0}({num})\x1b[0m", color.unwrap_or("0"))
                        },
                        Info::Fraction { amount, total } => {
                            format!("\x1b[33m{code}\u{00A0}({amount}/{total})\x1b[0m")
                        },
                    })
                    .collect();
                if codes.len() != shown_codes.len() {
                    shown_codes.push(format!("...and\u{00A0}{}\u{00A0}other(s)", codes.len() - shown_codes.len()));
                }
                shown_codes
            }

            let titles: &[(bool, &str, Vec<String>)] = if show_sources || show_translations { &[
                (false, "Languages",            format(languages, true, None)),
                (false, "Built-in providers",   format(builtin_providers, all, None)),
                (true,  "Retired providers",    format(retired_providers, true, None)),
                (true,  "Added providers",      format(added_providers, true, None)),
            ] } else { &[
                (false, "Languages",            format(languages, true, None)),
                (false, "Finished providers",   format(finished_providers, all, Some("32"))),
                (false, "Unfinished providers", format(unfinished_providers, all, None)),
                (false, "Failed providers",     format(failed_providers, true, Some("31"))),
                (true,  "Retired providers",    format(retired_providers, true, None)),
                (true,  "Added providers",      format(added_providers, true, None)),
            ] };
            for (i, (hide_when_empty, title, codes)) in titles.iter().enumerate() {
                if *hide_when_empty && codes.is_empty() { continue; }
                write!(console, "\x1b[1m{title}:\x1b[0m")?;
                if codes.is_empty() {
                    writeln!(console, " \x1b[3mnone\x1b[0m")?;
                } else {
                    write!(console, "\n  ")?;
                    wrapped_writeln(&mut console, &codes.join("  "), term_width, 2)?;
                }
                if i != titles.len() - 1 { writeln!(console)?; }
            }

            Ok(())
        }
    }
}

/// Returns the amount of chars in `s` that will be visible when printed to `stdout`.
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut is_ansi = false;
    for c in s.chars() {
        if !is_ansi && c as u8 == 0x1b { is_ansi = true; continue; }
        if is_ansi && c != '[' && c != ';' && !c.is_ascii_digit() { is_ansi = false; continue; }
        if is_ansi { continue; }
        len += 1;
    }
    len
}

/// Splits of the next line from `s`, returning it together with the remainder.
fn split_of_line(s: &str, width: usize, indent: usize) -> (&str, &str) {
    let width = width as i32;
    let indent = indent as i32;

    let mut index = 0;
    let mut length: i32 = -1; // Starts with -1 because all words start with +1
    for word in s.split(' ') {
        let visible_word_len = visible_len(word) as i32;
        if length + 1 + visible_word_len <= width {
            index += 1 + word.len();
            length += 1 + visible_word_len;
        } else if visible_word_len > width {
            index += 1 + word.len();
            length += 1 + visible_word_len;
            length -= width;
            length %= indent + width;
            length -= indent;
        } else {
            index -= 1; // Remove one because all words start with a +1
            return (s[0..index].trim_end(), s[index..].trim_start());
        }
    }
    (s, "")
}

/// Write `s` to `buf`, while ensuring that no line is longer than `max_width`,
/// and that every line is indented by `indent` spaces.
pub fn wrapped_writeln(mut out: impl Write, s: &str, max_width: usize, indent: usize) -> io::Result<()> {
    let indent_str = " ".repeat(indent);

    let mut first = true;
    for mut line in s.split('\n') {
        while !line.is_empty() {
            let (next_line, rest) = split_of_line(line, max_width - indent, indent);
            line = rest;

            if first { first = false; } else {
                write!(out, "{indent_str}")?;
            }
            writeln!(out, "{next_line}")?;
        }
    }

    Ok(())
}
