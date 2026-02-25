// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    io::{self, Write},
    mem::drop,
    sync::Arc,
};

use anyhow::{anyhow, bail};
use clap::{Parser, Subcommand};
use log::{info, warn};
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader, stdin},
    select,
    sync::Mutex,
};
use unic_langid::LanguageIdentifier;

use crate::{
    database::TranslationStore,
    providers::{ProviderCache, TranslationBundle},
    web_server::web_server,
    create_client,
    display_number,
    write_labeled_number_list,
    writeln_max_width,
};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the built-in web UI server.
    Run {
        /// Open the web UI in the default web browser.
        #[arg(long = "openbrowser")]
        open_browser: bool,
    },
    /// Start an interactive session.
    ///
    /// This allows for running multiple CLI commands without reloading the config file.
    #[command(alias = "cli")]
    Interactive,
    /// Stop an interactive session.
    #[command(alias = "quit", alias = "q")]
    Exit,
    /// Generate the translations of one or more providers.
    #[command(alias = "gen")]
    Generate {
        #[arg(value_delimiter = ',')]
        provider_ids: Vec<String>,

        /// Wich languages to generate for. [default: all languages that have already been generated for]
        #[arg(short, long, value_delimiter = ',')]
        languages: Option<Vec<LanguageIdentifier>>,
    },
    /// Remove the translations of one or more providers.
    Remove {
        #[arg(value_delimiter = ',')]
        provider_ids: Vec<String>,

        /// Only remove the translations to certain languages.
        #[arg(short, long, value_delimiter = ',')]
        languages: Option<Vec<LanguageIdentifier>>,
    },
    /// Print translations.
    Get {
        #[arg(value_delimiter = ',')]
        provider_ids: Vec<String>,

        /// Only print translations for certain languages.
        #[arg(short, long)]
        languages: Option<Vec<LanguageIdentifier>>,

        /// Only print up to a certain amount of translations.
        #[arg(long)]
        limit: Option<u8>,
    },
    /// Print information about the providers and translations.
    Stats {
        /// Print information about the translations [default]
        #[arg(long)]
        translations: bool,

        /// Print the translation providers.
        #[arg(long)]
        providers: bool,

        /// Print information about a translation provider.
        #[arg(short, long)]
        provider: Option<String>,

        /// Print the provider group names.
        #[arg(long)]
        group_names: bool,

        /// Print information about a provider group name.
        #[arg(short, long)]
        group_name: Option<String>,
    },
}

/// Perform the action associated with `command`.
pub async fn perform_command(
    command: Command,
    console: Arc<Mutex<StandardStream>>,
    term_width: usize,
    store: Arc<Mutex<TranslationStore>>,
) -> anyhow::Result<()> {
    match command {
        Command::Run { open_browser } => {
            if open_browser {
                info!("Opening web browser...");
                webbrowser::open("http://127.0.0.1:2013/")?;
            }

            info!("Starting web UI server at 'http://127.0.0.1:2013/'...");
            info!("Press 'q' to stop the server");
            let stop_listener = async {
                crossterm::terminal::enable_raw_mode()?;
                loop {
                    let c = stdin().read_u8().await?;
                    if c == b'q' {
                        break;
                    }
                    warn!("Press 'q' to stop the server");
                }
                crossterm::terminal::disable_raw_mode()?;
                Ok::<_, anyhow::Error>(())
            };
            select! {
                result = web_server(store.clone()) => {
                    result.map_err(|e| anyhow!("Could not start web UI server: {e}"))?
                }
                result = stop_listener => { result? }
            }

            Ok(())
        }
        Command::Interactive => {
            let mut red = ColorSpec::new();
            red.set_fg(Some(Color::Red));

            macro_rules! println_message {
                ($($arg:tt)*) => {
                    let mut console = console.lock().await;
                    console.set_color(&red)?;
                    let message = format!($($arg)*);
                    let message = message.trim_matches('\n');
                    writeln!(console, "{message}")?;
                    console.reset()?;
                    drop(console);
                };
            }

            println_message!(
                "you have started an interactive session; to stop the session, run 'exit'"
            );

            let mut stdin = BufReader::new(stdin());
            loop {
                let mut input = String::new();
                {
                    let mut console = console.lock().await;
                    write!(console, "> ")?;
                    console.flush()?;
                }
                stdin.read_line(&mut input).await?;
                if input.ends_with("\r\n") {
                    input.replace_range((input.len() - 2)..input.len(), "\n");
                }
                let input = match shell_words::split(&input) {
                    Ok(mut i) => {
                        let mut input = vec![String::new()];
                        input.append(&mut i);
                        input
                    }
                    Err(e) => {
                        println_message!("error: {e}");
                        continue;
                    }
                };

                #[derive(Debug, Parser)]
                struct InlineArgs {
                    #[command(subcommand)]
                    command: Command,
                }
                let args = match InlineArgs::try_parse_from(input) {
                    Ok(args) => args,
                    Err(e) => {
                        println_message!("{e}");
                        continue;
                    }
                };

                match args.command {
                    Command::Interactive => {
                        println_message!(
                            "subcommand 'interactive' cannot be used inside an interactive session"
                        );
                    }
                    Command::Exit => break,
                    command => {
                        let result = Box::pin(perform_command(
                            command,
                            console.clone(),
                            term_width,
                            store.clone(),
                        ))
                        .await;
                        if let Err(e) = result {
                            println_message!("error: {e}");
                        }
                    }
                }
            }
            Ok(())
        }
        Command::Exit => {
            bail!("subcommand 'exit' cannot be used outside of an interactive session")
        }
        Command::Generate {
            provider_ids,
            languages,
        } => {
            let mut store = store.lock().await;

            let languages =
                languages.unwrap_or_else(|| store.languages().into_iter().cloned().collect());
            if languages.is_empty() {
                bail!("No languages are specified");
            }

            let client = create_client()?;

            let errors = store
                .generate(languages, provider_ids, false, client)
                .await?;
            if errors.values().any(|error| error.is_none()) {
                store.save_translations()?;
            }

            Ok(())
        }
        Command::Remove {
            provider_ids,
            languages,
        } => {
            let mut store = store.lock().await;

            for provider_id in provider_ids {
                if let Some(languages) = &languages {
                    let Some(translations) = store.provider_caches.get_mut(&provider_id) else {
                        match store.provider(&provider_id) {
                            Some(_) => warn!("Scope '{provider_id}' has no generated translations"),
                            None => warn!("Scope '{provider_id}' was not found"),
                        }
                        continue;
                    };
                    for lang_id in languages {
                        let removed_translations = translations
                            .translation_bundles_mut()
                            .map(|bundle| bundle.remove(lang_id))
                            .fold(None, |acc, translations| {
                                match (acc, translations.map(|t| t.is_some())) {
                                    (Some(true), _) | (_, Some(true)) => Some(true),
                                    (Some(false), _) | (_, Some(false)) => Some(false),
                                    (None, None) => None,
                                }
                            });
                        match removed_translations {
                            Some(true) => {
                                info!("Removed language '{lang_id}' from scope '{provider_id}'")
                            }
                            Some(false) => {
                                warn!(
                                    "Scope '{provider_id}' has no translations for language '{lang_id}'"
                                )
                            }
                            None => warn!(
                                "Scope '{provider_id}' has not generated language '{lang_id}'"
                            ),
                        }
                    }
                } else {
                    match store.provider_caches.remove(&provider_id) {
                        Some(_) => info!("Removed scope '{provider_id}'"),
                        None => match store.provider(&provider_id) {
                            Some(_) => warn!("Scope '{provider_id}' has no generated translations"),
                            None => warn!("Scope '{provider_id}' was not found"),
                        },
                    }
                }
            }
            store.save_translations()?;

            Ok(())
        }
        Command::Get {
            provider_ids,
            languages,
            limit,
        } => {
            let store = store.lock().await;
            let mut stdout = console.lock().await;

            let mut color_green = ColorSpec::new();
            color_green.set_fg(Some(Color::Green));
            let color_none = ColorSpec::new();

            for provider_id in provider_ids {
                store
                    .translations()
                    .filter(|(scope, lang_id, _)| {
                        if languages.as_ref().is_some_and(|ls| !ls.contains(lang_id)) {
                            return false;
                        }
                        **scope == provider_id
                    })
                    .take(limit.unwrap_or(u8::MAX) as usize)
                    .map(|(.., translation)| translation)
                    .for_each(|translation| {
                        stdout.set_color(&color_green).unwrap();
                        write!(stdout, "original:   ").unwrap();
                        stdout.set_color(&color_none).unwrap();
                        writeln_max_width(io::stdout(), &translation.original, 13, 13, term_width)
                            .unwrap();
                        stdout.set_color(&color_green).unwrap();
                        write!(stdout, "translation:").unwrap();
                        stdout.set_color(&color_none).unwrap();
                        writeln_max_width(
                            io::stdout(),
                            &translation.translation,
                            13,
                            13,
                            term_width,
                        )
                        .unwrap();
                        if let Some(comment) = &translation.comment {
                            stdout.set_color(&color_green).unwrap();
                            write!(stdout, "comment:").unwrap();
                            stdout.set_color(&color_none).unwrap();
                            writeln_max_width(io::stdout(), comment, 9, 9, term_width).unwrap();
                        }
                        writeln!(stdout).unwrap();
                    });
            }

            Ok(())
        }

        Command::Stats {
            provider,
            mut translations,
            group_name,
            providers,
            group_names,
        } => {
            // Make `--translations` the default so there is always output.
            if provider.is_none() && group_name.is_none() && !providers && !group_names {
                translations = true;
            }

            let store = store.lock().await;
            let mut console = console.lock().await;

            let mut highlighted = ColorSpec::new();
            highlighted.set_fg(Some(Color::White));
            highlighted.set_intense(true);

            macro_rules! label_println {
                ($label:literal, value: $($value:tt)*) => {
                    console.set_color(&highlighted)?;
                    write!(console, "{}", $label)?;
                    console.reset()?;
                    writeln!(console, $($value)*)?;
                };
                (return => $($label:tt)*) => {{
                    let label = format!($($label)*);
                    console.set_color(&highlighted)?;
                    write!(console, "{label}")?;
                    console.reset()?;
                    label
                }};
                ($($label:tt)*) => {
                    console.set_color(&highlighted)?;
                    writeln!(console, $($label)*)?;
                    console.reset()?;
                };
            }

            fn print_languages<'a>(
                console: impl Write,
                indent: usize,
                translation_bundles: impl Iterator<Item = &'a TranslationBundle>,
            ) -> anyhow::Result<()> {
                let translation_counts: Vec<(String, Option<usize>)> = translation_bundles
                    .flatten()
                    .fold(HashMap::new(), |mut map, (lang_id, translations)| {
                        let count = map.entry(lang_id).or_insert(None);
                        let Some(translations) = translations.as_ref() else {
                            return map;
                        };
                        if count.is_none() {
                            *count = Some(0);
                        }
                        let Some(count) = count else {
                            unreachable!();
                        };
                        *count += translations.len();
                        map
                    })
                    .into_iter()
                    .map(|(lang_id, count)| (lang_id.to_string(), count))
                    .collect();

                write_labeled_number_list(console, indent, translation_counts)?;

                Ok(())
            }

            if translations {
                label_println!("Translations: ", value: "{}", display_number(store.translations().count()));

                label_println!(
                    "  By language ({}):",
                    display_number(store.languages().len())
                );
                print_languages(
                    &mut *console,
                    4,
                    store
                        .provider_caches
                        .values()
                        .flat_map(|provider_cache| provider_cache.translation_bundles()),
                )?;

                let scope_counts: Vec<(&str, Option<usize>)> = store
                    .providers()
                    .fold(HashMap::new(), |mut map, provider| {
                        let count = map
                            .entry(provider.group_name().unwrap_or(provider.name()))
                            .or_insert(None);
                        let Some(provider_cache) = store.provider_caches.get(provider.id()) else {
                            return map;
                        };
                        if count.is_none() {
                            *count = Some(0);
                        }
                        let Some(count) = count else {
                            unreachable!();
                        };
                        *count += provider_cache
                            .translation_bundles()
                            .flat_map(|bundle| bundle.values())
                            .filter_map(|translations| translations.as_ref())
                            .map(|translations| translations.len())
                            .sum::<usize>();
                        map
                    })
                    .into_iter()
                    .collect();
                label_println!("  By scope ({}):", display_number(scope_counts.len()));
                write_labeled_number_list(&mut *console, 4, scope_counts)?;
            }

            if providers {
                let rest: Vec<_> = store.providers().collect();

                let (mut temporary_providers, rest): (Vec<_>, Vec<_>) =
                    rest.into_iter().partition(|provider| provider.temporary());
                temporary_providers.sort_by_key(|provider| provider.id());

                let (mut not_generated_providers, rest): (Vec<_>, Vec<_>) = rest
                    .into_iter()
                    .partition(|provider| !store.provider_caches.contains_key(provider.id()));
                not_generated_providers.sort_by_key(|provider| provider.id());

                let (mut not_finished, rest): (Vec<_>, Vec<_>) =
                    rest.into_iter().partition(|provider| {
                        let Some(ProviderCache::Multiple(multiple)) =
                            store.provider_caches.get(provider.id())
                        else {
                            return false;
                        };
                        !multiple.finished
                    });
                not_finished.sort_by_key(|provider| provider.id());

                let (mut empty_providers, mut rest): (Vec<_>, Vec<_>) =
                    rest.into_iter().partition(|provider| {
                        let Some(provider_cache) = store.provider_caches.get(provider.id()) else {
                            return false;
                        };
                        provider_cache
                            .translation_bundles()
                            .flat_map(|bundle| bundle.values())
                            .all(|translations| {
                                translations
                                    .as_ref()
                                    .is_none_or(|translations| translations.is_empty())
                            })
                    });
                empty_providers.sort_by_key(|provider| provider.id());

                rest.sort_by_key(|provider| provider.id());

                label_println!("Providers ({}):", display_number(store.providers().count()));

                let categories = [
                    ("Temporary", temporary_providers),
                    ("Not generated", not_generated_providers),
                    ("Generated, but not finished", not_finished),
                    ("Generated, with no translations", empty_providers),
                    ("Generated, with translations", rest),
                ];

                for (label, providers) in categories {
                    let label = label_println!(return => "  {label} ({}):", display_number(providers.len()));

                    let providers = providers
                        .iter()
                        .fold(String::new(), |acc, provider| acc + provider.id() + ", ");
                    let providers = providers.trim_end_matches(", ");

                    writeln_max_width(
                        &mut *console,
                        providers,
                        label.len(),
                        label.len(),
                        term_width,
                    )?;
                }
            }

            if let Some(provider_id_or_name) = provider {
                let provider = if let Some(provider) = store.provider(&provider_id_or_name) {
                    provider
                } else if let Some(provider) = store
                    .providers()
                    .find(|provider| provider.name() == provider_id_or_name)
                {
                    provider
                } else {
                    bail!("No provider with id or name '{provider_id_or_name}'");
                };
                let provider_cache = store.provider_caches.get(provider.id());

                label_println!("Provider '{}':", provider.id());
                label_println!("  Name: ", value: "'{}'", provider.name());
                label_println!("  Group name: ", value: "{}", provider.group_name()
                    .map_or(Cow::Borrowed("none"), |group_name| Cow::Owned(format!("'{group_name}'"))),
                );
                label_println!("  Cache type: ", value: "{}", match provider_cache {
                    Some(ProviderCache::Single(_)) => "single",
                    Some(ProviderCache::Multiple(_)) => "multiple",
                    None => "none",
                });

                if let Some(ProviderCache::Multiple(multiple)) = &provider_cache {
                    label_println!("  Finished: ", value: "{}", multiple.finished);
                    label_println!("  Translation bundles: ", value: "{}",
                        display_number(multiple.translation_bundles.len()),
                    );
                }

                if let Some(provider_cache) = &provider_cache {
                    let translation_count = provider_cache
                        .translation_bundles()
                        .flat_map(|bundle| bundle.values())
                        .filter_map(|translations| translations.as_ref())
                        .map(|translations| translations.len())
                        .sum();
                    label_println!("  Translations: ", value: "{}", display_number(translation_count));

                    print_languages(&mut *console, 4, provider_cache.translation_bundles())?;
                }
            }

            if group_names {
                let mut group_names = store
                    .providers()
                    .filter_map(|provider| provider.group_name())
                    .collect::<HashSet<&str>>()
                    .into_iter()
                    .collect::<Vec<&str>>();
                group_names.sort();

                label_println!("Group names ({}):", group_names.len());

                for group_name in group_names {
                    writeln!(console, "  {group_name}")?;
                }
            }

            if let Some(group_name) = group_name {
                let providers: Vec<_> = store
                    .providers()
                    .filter(|provider| provider.group_name() == Some(group_name.as_str()))
                    .collect();
                if providers.is_empty() {
                    bail!("No providers has the group name '{group_name}'");
                };

                label_println!("Group name '{group_name}':");

                let label = label_println!(return => "  Providers ({}):", providers.len());

                let mut provider_ids: Vec<&str> =
                    providers.iter().map(|provider| provider.id()).collect();
                provider_ids.sort();
                writeln_max_width(
                    &mut *console,
                    &provider_ids.join(", "),
                    label.len(),
                    label.len(),
                    term_width,
                )?;

                let translation_count = providers
                    .iter()
                    .filter_map(|provider| store.provider_caches.get(provider.id()))
                    .flat_map(|provider_cache| provider_cache.translation_bundles())
                    .flat_map(|bundle| bundle.values())
                    .filter_map(|translations| translations.as_ref())
                    .map(|translations| translations.len())
                    .sum();
                label_println!("  Translations: ", value: "{}", display_number(translation_count));

                print_languages(
                    &mut *console,
                    4,
                    providers
                        .iter()
                        .filter_map(|provider| store.provider_caches.get(provider.id()))
                        .flat_map(|provider_cache| provider_cache.translation_bundles()),
                )?;
            }

            Ok(())
        }
    }
}
