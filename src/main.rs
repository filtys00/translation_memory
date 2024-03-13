// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod web_server;

use std::{
    io::{self, Write},
    path::Path,
};

use clap::Parser;
use env_logger::fmt::Color;
use log::{error, info, Level, LevelFilter};
use translation_memory::TranslationStore;
use unic_langid::LanguageIdentifier;

use self::web_server::web_server;

const CACHE_PATH: &str = "translations.bin.xz";
const CONFIG_PATH: &str = "translations.toml";

#[derive(Debug, Parser)]
struct Args {
    /// Write all trace logs
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Don't open the system web browser when the web server starts
    #[arg(long, conflicts_with = "generate")]
    no_browser: bool,

    /// Generate one or more providers
    #[arg(long, value_delimiter = ',')]
    generate: Vec<String>,

    /// Languages to generate for
    #[arg(long, value_delimiter = ',', requires = "generate")]
    language: Vec<LanguageIdentifier>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let term_width = termsize::get().map_or(80, |size| size.cols as usize);
    env_logger::builder()
        .filter_module(
            env!("CARGO_PKG_NAME"),
            if args.verbose {
                LevelFilter::max()
            } else {
                LevelFilter::Debug
            },
        )
        .format(move |buf, record| {
            let mut dimmed_style = buf.style();
            dimmed_style.set_color(Color::Black);
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
        })
        .init();

    let mut store = TranslationStore::default();

    if Path::new(CACHE_PATH).exists() {
        info!("Reading cached translations from '{CACHE_PATH}'...");
        store.load_translations(CACHE_PATH).unwrap();
    }

    if Path::new(CONFIG_PATH).exists() {
        info!("Reading config from '{CONFIG_PATH}'...");
        store.load_config(CONFIG_PATH).unwrap();
    }

    if !args.generate.is_empty() {
        let lang_ids = if args.language.is_empty() {
            store.languages().into_iter().cloned().collect()
        } else {
            args.language
        };
        if lang_ids.is_empty() {
            error!("No languages are specified");
        }
        if let Err(e) = store.generate(lang_ids, args.generate, false).await {
            error!("{e}");
        }
        if let Err(e) = store.write_to_file(CACHE_PATH) {
            error!("{e}");
        }
        return;
    }

    if !args.no_browser {
        info!("Opening web browser...");
        webbrowser::open("http://127.0.0.1:2013/").unwrap();
    }

    info!("Starting web server...");
    if let Err(e) = web_server(store).await {
        error!("Could not start web server: {e}");
        return;
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
