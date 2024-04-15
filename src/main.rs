// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod web_server;

use std::{
    io::{self, Write},
    path::Path,
};

use clap::Parser;
use log::{error, info, warn, Level, LevelFilter};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use translation_memory::TranslationStore;
use unic_langid::LanguageIdentifier;

use self::web_server::web_server;

const DEFAULT_CACHE_PATH: &str = "translations.bin.gz";
const CONFIG_PATH: &str = "translations.toml";

#[derive(Debug, Parser)]
struct Args {
    /// Output all logs, including trace logs.
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Do not open the system web browser when the web server starts.
    #[arg(long = "nobrowser", conflicts_with = "statement")]
    no_browser: bool,

    #[arg(long, group = "statement")]
    status: bool,

    #[arg(long, value_delimiter = ',', group = "statement")]
    get: Vec<String>,

    #[arg(long, requires = "get")]
    limit: Option<u8>,

    #[arg(long, value_delimiter = ',', group = "statement")]
    generate: Vec<String>,

    #[arg(long, value_delimiter = ',', group = "statement")]
    remove: Vec<String>,

    #[arg(long = "lang", value_delimiter = ',', requires = "statement")]
    languages: Vec<LanguageIdentifier>,
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
                LevelFilter::Info
            },
        )
        .format(move |buf, record| {
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
        })
        .init();

    let mut store = TranslationStore::new(DEFAULT_CACHE_PATH.into());

    if Path::new(CONFIG_PATH).exists() {
        info!("Reading config from '{CONFIG_PATH}'...");
        store.load_config(CONFIG_PATH).unwrap();
    }

    info!("Reading cached translations from '{DEFAULT_CACHE_PATH}'...");
    store.load_translations().unwrap();

    if !args.remove.is_empty() {
        for name in &args.remove {
            if args.languages.is_empty() {
                match store.translations.remove(name) {
                    Some(_) => info!("Removed scope '{name}'"),
                    None => match store.provider(name) {
                        Some(_) => warn!("Scope '{name}' has no generated translations"),
                        None => warn!("Scope '{name}' was not found"),
                    },
                }
            } else {
                let Some(scope) = store.translations.get_mut(name) else {
                    match store.provider(name) {
                        Some(_) => warn!("Scope '{name}' has no generated translations"),
                        None => warn!("Scope '{name}' was not found"),
                    }
                    continue;
                };
                for lang in &args.languages {
                    match scope.remove(lang) {
                        Some(Some(_)) => info!("Removed language '{lang}' from scope '{name}'"),
                        Some(None) => {
                            warn!("Scope '{name}' has no translations for language '{lang}'")
                        }
                        None => warn!("Scope '{name}' has not generated language '{lang}'"),
                    }
                }
            }
        }
        if let Err(e) = store.save_translations() {
            error!("{e}");
        }
    }

    if !args.generate.is_empty() {
        let lang_ids = if args.languages.is_empty() {
            store.languages().into_iter().cloned().collect()
        } else {
            args.languages.clone()
        };
        if lang_ids.is_empty() {
            error!("No languages are specified");
        }
        if let Err(e) = store.generate(lang_ids, args.generate.clone(), false).await {
            error!("{e}");
        }
        if let Err(e) = store.save_translations() {
            error!("{e}");
        }
    }

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut color_green = ColorSpec::new();
    color_green.set_fg(Some(Color::Green));
    let color_none = ColorSpec::new();

    if !args.get.is_empty() {
        for name in &args.get {
            store
                .iter()
                .filter(|(scope, lang_id, _)| {
                    if !args.languages.is_empty() && !args.languages.contains(lang_id) {
                        return false;
                    }
                    *scope == name
                })
                .take(args.limit.unwrap_or(u8::MAX) as usize)
                .map(|(.., translation)| translation)
                .for_each(|translation| {
                    stdout.set_color(&color_green).unwrap();
                    write!(&mut stdout, "original:   ").unwrap();
                    stdout.set_color(&color_none).unwrap();
                    writeln_max_width(io::stdout(), &translation.original, 13, 13, term_width)
                        .unwrap();
                    stdout.set_color(&color_green).unwrap();
                    write!(&mut stdout, "translation:").unwrap();
                    stdout.set_color(&color_none).unwrap();
                    writeln_max_width(io::stdout(), &translation.translation, 13, 13, term_width)
                        .unwrap();
                    if let Some(comment) = &translation.comment {
                        stdout.set_color(&color_green).unwrap();
                        write!(&mut stdout, "comment:").unwrap();
                        stdout.set_color(&color_none).unwrap();
                        writeln_max_width(io::stdout(), comment, 9, 9, term_width).unwrap();
                    }
                    writeln!(&mut stdout).unwrap();
                });
        }
    }

    if args.status {
        println!("In total {} scopes", store.providers().len());
        println!(
            "  generated: {} scopes",
            store
                .providers()
                .iter()
                .filter(|provider| store.translations.contains_key(provider.id()))
                .count()
        );
        println!(
            "    empty: {} scopes",
            store
                .providers()
                .iter()
                .filter(|provider| store
                    .translations
                    .get(provider.id())
                    .map_or(false, |scope| scope
                        .values()
                        .all(|t| t.as_ref().map_or(true, |t| t.is_empty()))))
                .count()
        );
        for provider in store.providers().iter().filter(|provider| {
            store
                .translations
                .get(provider.id())
                .map_or(false, |scope| {
                    scope
                        .values()
                        .all(|t| t.as_ref().map_or(true, |t| t.is_empty()))
                })
        }) {
            println!("      {}", provider.id());
        }
        println!(
            "  not generated: {} scopes",
            store
                .providers()
                .iter()
                .filter(|provider| !store.translations.contains_key(provider.id()))
                .count()
        );
        for provider in store
            .providers()
            .iter()
            .filter(|provider| !store.translations.contains_key(provider.id()))
        {
            println!("    {}", provider.id());
        }

        println!("In total {} translations", store.iter().count());
        for lang in store.languages() {
            println!(
                "  {lang}: {} translations, {} / {} scopes",
                store.iter().filter(|(_, l, _)| *l == lang).count(),
                store
                    .translations
                    .values()
                    .filter(|scope| scope.get(lang).map_or(false, |scope| scope.is_some()))
                    .count(),
                store.translations.len(),
            );
            if store
                .translations
                .values()
                .filter(|scope| scope.get(lang).map_or(false, |scope| scope.is_some()))
                .count()
                <= 10
            {
                for (name, translations) in store
                    .translations
                    .iter()
                    .filter(|(_, scope)| scope.get(lang).map_or(false, |scope| scope.is_some()))
                {
                    println!(
                        "    {name}: {} translations",
                        translations
                            .get(lang)
                            .map_or(0, |t| t.as_ref().map_or(0, |t| t.len()))
                    );
                }
            }
        }

        if !args.languages.is_empty() {
            println!();
            println!(
                "Languages {}",
                args.languages
                    .iter()
                    .map(|lang_id| lang_id.to_string())
                    .reduce(|acc, lang_id| acc + ", " + &lang_id)
                    .unwrap_or_else(String::new)
            );
            let mut counts = Vec::with_capacity(store.translations.len());
            for (name, translations) in &store.translations {
                let mut count = 0;
                for lang_id in &args.languages {
                    let Some(Some(translations)) = translations.get(lang_id) else {
                        continue;
                    };
                    count += translations.len();
                }
                if count > 0 {
                    counts.push((name, count));
                }
            }
            counts.sort_by_key(|(name, count)| (*count, *name));
            for (name, count) in counts {
                println!("  {name}: {count}");
            }
        }
    }

    if !args.remove.is_empty() || !args.generate.is_empty() || !args.get.is_empty() || args.status {
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
