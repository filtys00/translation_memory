// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod web_server;

use std::{
    io::{self, Write},
    path::Path,
    sync::Arc,
};

use anyhow::bail;
use clap::{Parser, Subcommand, ValueEnum};
use log::{error, info, warn, Level, LevelFilter};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    select,
    sync::Mutex,
};
use translation_memory::TranslationStore;
use unic_langid::LanguageIdentifier;

use self::web_server::web_server;

const DEFAULT_CACHE_PATH: &str = "translations.bin";
const CONFIG_PATH: &str = "translations.toml";

#[derive(Clone, Debug, ValueEnum)]
enum VerboseMode {
    None,
    Info,
    Debug,
    WebServer,
    Providers,
    All,
}

#[derive(Debug, Parser)]
struct Args {
    /// Wich log messages to print.
    #[arg(
        long, global = true,
        value_enum, value_name = "MODE",
        default_value_t = VerboseMode::Info,
        default_missing_value = "all",
        num_args = 0..=1, require_equals = true,
    )]
    verbose: VerboseMode,

    /// Ignore the config file [path: translations.toml].
    #[arg(long = "noconfig", global = true)]
    no_config: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the built-in web UI server.
    Run {
        /// Do not open the system web browser when the web server starts.
        #[arg(long = "nobrowser")]
        no_browser: bool,
    },
    /// Stop the built-in web UI server.
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
    /// Print statistics about the collected translations.
    Status {
        /// Print statistics about the translations of one or more languages.
        #[arg(short, long)]
        languages: Option<Vec<LanguageIdentifier>>,
    },
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let term_width = termsize::get().map_or(80, |size| size.cols as usize);
    let mut logger = env_logger::builder();
    logger.format(move |buf, record| {
        let mut dimmed_style = buf.style();
        dimmed_style.set_color(env_logger::fmt::Color::Black);
        dimmed_style.set_intense(true);

        write!(
            buf,
            "{}{} {}{}",
            dimmed_style.value('['),
            buf.default_styled_level(record.level()),
            record.target(),
            dimmed_style.value(']'),
        )?;

        let level_length = match record.level() {
            Level::Error | Level::Debug | Level::Trace => 5,
            Level::Warn | Level::Info => 4,
        };

        writeln_max_width(
            buf,
            &record.args().to_string(),
            3 + level_length + record.target().len(),
            1 + level_length,
            term_width,
        )
    });
    match args.verbose {
        VerboseMode::None => {
            logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Off);
        }
        VerboseMode::Info => {
            logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Info);
        }
        VerboseMode::Debug => {
            logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Debug);
        }
        VerboseMode::WebServer => {
            logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Info);
            logger.filter_module(
                concat!(env!("CARGO_PKG_NAME"), "::web_server"),
                LevelFilter::max(),
            );
        }
        VerboseMode::Providers => {
            logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Info);
            logger.filter_module(
                concat!(env!("CARGO_PKG_NAME"), "::providers"),
                LevelFilter::max(),
            );
        }
        VerboseMode::All => {
            logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::max());
        }
    }
    logger.init();

    let mut store = TranslationStore::new(DEFAULT_CACHE_PATH.into());

    if !args.no_config && Path::new(CONFIG_PATH).exists() {
        info!("Reading config from '{CONFIG_PATH}'...");
        if let Err(e) = store.load_config(CONFIG_PATH) {
            error!("Could not load config file ({CONFIG_PATH}): {e}");
            return;
        }
    }

    info!("Reading cached translations...");
    if let Err(e) = store.load_translations() {
        error!("Could not load cached translations: {e}");
        return;
    }

    let store = Arc::new(Mutex::new(store));
    let console = Arc::new(Mutex::new(StandardStream::stderr(ColorChoice::Auto)));

    let command = args
        .command
        .unwrap_or_else(|| Command::Run { no_browser: false });
    match command {
        Command::Run { no_browser } => {
            if !no_browser {
                info!("Opening web browser...");
                webbrowser::open("http://127.0.0.1:2013/").unwrap();
            }

            info!("Starting web UI server at 'http://127.0.0.1:2013/'...");
            select! {
                e = web_server(store.clone()) => {
                    if let Err(e) = e {
                        error!("Could not start web UI server: {e}");
                    }
                }
                e = inline_cli_listener(console, term_width, store.clone()) => {
                    if let Err(e) = e {
                        error!("Inline CLI: {e}");
                    }
                }
            }
        }
        command => {
            let result = perform_command(command, console, term_width, store).await;
            if let Err(e) = result {
                error!("{e}");
                return;
            }
        }
    }
}

/// Listen for commands on `stdin()` and perform them with `perform_command`.
async fn inline_cli_listener(
    console: Arc<Mutex<StandardStream>>,
    term_width: usize,
    store: Arc<Mutex<TranslationStore>>,
) -> anyhow::Result<()> {
    let mut red = ColorSpec::new();
    red.set_fg(Some(Color::Red));

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
                let mut console = console.lock().await;
                console.set_color(&red)?;
                writeln!(console, "error: {e}")?;
                console.reset()?;
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
                let mut console = console.lock().await;
                console.set_color(&red)?;
                writeln!(console, "{e}")?;
                console.reset()?;
                continue;
            }
        };

        if let Command::Exit = args.command {
            break;
        }
        let result =
            perform_command(args.command, console.clone(), term_width, store.clone()).await;
        if let Err(e) = result {
            let mut console = console.lock().await;
            console.set_color(&red)?;
            writeln!(console, "error: {e}")?;
            writeln!(console)?;
            console.reset()?;
        }
    }
    Ok(())
}

/// Perform the action associated with `command`.
async fn perform_command(
    command: Command,
    console: Arc<Mutex<StandardStream>>,
    term_width: usize,
    store: Arc<Mutex<TranslationStore>>,
) -> anyhow::Result<()> {
    match command {
        Command::Run { .. } => {
            bail!("subcommand 'run' cannot be used when the server is already running")
        }
        Command::Exit => {
            bail!("subcommand 'exit' cannot be used when the server is not yet running")
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

            let errors = store.generate(languages, provider_ids, false).await?;
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
                                warn!("Scope '{provider_id}' has no translations for language '{lang_id}'")
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
                        if let Some(languages) = &languages {
                            if !languages.contains(lang_id) {
                                return false;
                            }
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
        Command::Status { languages } => {
            let store = store.lock().await;

            println!("In total {} scopes", store.providers().len());
            println!(
                "  generated: {} scopes",
                store
                    .providers()
                    .iter()
                    .filter(|provider| store.provider_caches.contains_key(provider.id()))
                    .count()
            );
            let empty = store.providers().iter().filter(|provider| {
                store
                    .provider_caches
                    .get(provider.id())
                    .map_or(false, |provider_cache| {
                        provider_cache
                            .translation_bundles()
                            .flat_map(|bundle| bundle.values())
                            .filter_map(|translations| translations.as_ref())
                            .all(|translations| translations.is_empty())
                    })
            });
            println!("    empty: {} scopes", empty.clone().count());
            for provider in empty {
                println!("      {}", provider.id());
            }
            let not_generated = store
                .providers()
                .iter()
                .filter(|provider| !store.provider_caches.contains_key(provider.id()));
            println!("  not generated: {} scopes", not_generated.clone().count());
            for provider in not_generated {
                println!("    {}", provider.id());
            }

            println!("In total {} translations", store.translations().count());
            for lang_id in store.languages() {
                let provider_caches = store.provider_caches.iter().filter(|(_, provider_cache)| {
                    provider_cache
                        .translation_bundles()
                        .filter_map(|bundle| bundle.get(lang_id))
                        .any(|translations| translations.is_some())
                });
                println!(
                    "  {lang_id}: {} translations, {} / {} scopes",
                    store
                        .translations()
                        .filter(|(_, l, _)| *l == lang_id)
                        .count(),
                    provider_caches.clone().count(),
                    store.providers().len(),
                );
                if provider_caches.clone().count() <= 10 {
                    for (provider_id, provider_cache) in provider_caches {
                        println!(
                            "    {provider_id}: {} translations",
                            provider_cache
                                .translation_bundles()
                                .filter_map(|bundle| bundle.get(lang_id))
                                .filter_map(|translations| translations.as_ref())
                                .flatten()
                                .count(),
                        );
                    }
                }
            }

            if let Some(languages) = languages {
                println!();
                println!(
                    "Languages {}",
                    languages
                        .iter()
                        .map(|lang_id| lang_id.to_string())
                        .reduce(|acc, lang_id| acc + ", " + &lang_id)
                        .unwrap_or_default()
                );
                let mut counts = Vec::with_capacity(store.provider_caches.len());
                for (provider_id, provider_cache) in &store.provider_caches {
                    let mut count = 0;
                    for language in &languages {
                        count += provider_cache
                            .translation_bundles()
                            .filter_map(|bundle| bundle.get(language))
                            .filter_map(|translations| translations.as_ref())
                            .flatten()
                            .count();
                    }
                    if count > 0 {
                        counts.push((provider_id, count));
                    }
                }
                counts.sort_by_key(|(provider_id, count)| (*count, *provider_id));
                for (provider_id, count) in counts {
                    println!("  {provider_id}: {count}");
                }
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
fn writeln_max_width(
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
