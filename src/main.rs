// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod database;
mod providers;
mod cli;
mod web_server;

use std::{io::{self, Write}, path::Path, process::ExitCode, sync::Arc};

use clap::{Parser, ValueEnum};
use log::{Level, LevelFilter, error, info};
use reqwest::Client;
use termcolor::{ColorChoice, StandardStream};
use tokio::sync::Mutex;

use crate::{database::TranslationStore, cli::{Command, perform_command}};

const DEFAULT_CACHE_PATH: &str = "translations.sqlite";
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

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    let term_width = crossterm::terminal::size().map_or(80, |(cols, _)| cols as usize);
    let mut logger = env_logger::builder();
    logger.format(move |buf, record| {
        let dimmed_style = env_logger::fmt::style::Style::new()
            .fg_color(Some(env_logger::fmt::style::Color::Ansi(
                env_logger::fmt::style::AnsiColor::Black,
            )))
            .bold();

        dimmed_style.write_to(buf)?;
        write!(buf, "[")?;
        dimmed_style.write_reset_to(buf)?;

        let level_style = buf.default_level_style(record.level());
        level_style.write_to(buf)?;
        write!(buf, "{}", record.level().as_str())?;
        level_style.write_reset_to(buf)?;

        write!(buf, " {}", record.target())?;

        dimmed_style.write_to(buf)?;
        write!(buf, "]")?;
        dimmed_style.write_reset_to(buf)?;

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
            return ExitCode::FAILURE;
        }
    }

    info!("Reading cached translations...");
    if let Err(e) = store.load_translations() {
        error!("Could not load cached translations: {e}");
        return ExitCode::FAILURE;
    }

    let store = Arc::new(Mutex::new(store));
    let console = Arc::new(Mutex::new(StandardStream::stderr(ColorChoice::Auto)));

    let command = args.command.unwrap_or(Command::Run { open_browser: true });
    let result = perform_command(command, console, term_width, store).await;
    if let Err(e) = result {
        error!("{e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// Returns a properly configured `Client`.
fn create_client() -> anyhow::Result<Client> {
    let client = Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
        ))
        .build()?;
    Ok(client)
}

/// Converts `number` to `String` with every three digits seperated by non-breaking spaces.
fn display_number(number: usize) -> String {
    let number = number.to_string();
    if number.len() < 4 {
        return number;
    }

    let mut new_number = String::new();
    for (i, c) in number.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            new_number.push('\u{00A0}');
        }
        new_number.push(c);
    }
    new_number.chars().rev().collect()
}

/// Write `list` of labels and optional numbers to `buf`.
/// Each entry is written on it's own line with an indent of `indent` spaces.
/// The entries are aligned vertically, and the numbers are formatted to be more human-readable.
///
/// `list` is sorted by number first, and then label.
///
/// # Example
///
/// ```
/// #use std::io::stderr;
/// #fn main() {
/// let list = vec![
///     ("first entry", None),
///     ("second entry", Some(2167)),
///     ("third entry", Some(5422)),
///     ("fourth entry", Some(2167)),
/// ];
/// writeln!("Entries:").unwrap();
/// write_labeled_number_list(stderr(), 2).unwrap();
/// #}
/// ```
///
/// Expected output:
/// ```terminal
/// Entries:
///    third entry: 5 422
///    first entry: 3 773
///   fourth entry: 2 167
///   second entry: 2 167
/// ```
fn write_labeled_number_list(
    mut buf: impl Write,
    indent: usize,
    mut list: Vec<(impl AsRef<str>, Option<usize>)>,
) -> anyhow::Result<()> {
    list.sort_by(|(lang_id_a, count_a), (lang_id_b, count_b)| {
        let ordering = count_a.cmp(count_b).reverse();
        if ordering.is_eq() {
            lang_id_a.as_ref().cmp(lang_id_b.as_ref())
        } else {
            ordering
        }
    });

    let label_max_length = list
        .iter()
        .map(|(label, _)| label.as_ref().chars().count())
        .max()
        .unwrap_or(0);
    let value_max_length = list
        .iter()
        .map(|(_, value)| {
            value
                .as_ref()
                .map(|value| display_number(*value).chars().count())
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);

    for (label, value) in &*list {
        let value = value.as_ref().map(|value| display_number(*value));

        write!(
            buf,
            "{}",
            " ".repeat(indent + (label_max_length - label.as_ref().chars().count()))
        )?;
        write!(buf, "{}: ", label.as_ref())?;

        if let Some(value) = value {
            write!(
                buf,
                "{}",
                " ".repeat(value_max_length - value.chars().count())
            )?;
            writeln!(buf, "{value}")?;
        } else {
            writeln!(buf, "none")?;
        }
    }
    Ok(())
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
