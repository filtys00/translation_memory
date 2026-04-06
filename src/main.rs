// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod database;
mod providers;
mod cli;
mod web_server;

use std::{io::Write, process::ExitCode};

use clap::{ArgAction, Parser};
use log::{Level, LevelFilter, error, info};
use rusqlite::Connection;
use termcolor::{ColorChoice, StandardStream};

use crate::{cli::{Command, perform_command, writeln_max_width}, database::TranslationStore};

const DEFAULT_CACHE_PATH: &str = "translations.sqlite";

#[derive(Parser)]
#[command(version)]
struct Args {
    /// How verbose the logging should be [possible repeats: 2]
    #[arg(global = true, short, long, action = ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Command>,
}

fn main() -> ExitCode {
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
    match args.verbose { // Only filter module env!("CARGO_PKG_NAME") to prevent logs from dependencies.
        0 => { logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Info); }
        1 => { logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Debug); }
        _ => { logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::max()); }
    }
    logger.init();

    info!("Reading translations from '{DEFAULT_CACHE_PATH}'...");
    let connection = match Connection::open(DEFAULT_CACHE_PATH) {
        Ok(connection) => connection,
        Err(e) => {
            error!("Could not connect to database: {e}");
            return ExitCode::FAILURE;
        }
    };
    let store = match TranslationStore::open(connection) {
        Ok(store) => store,
        Err(e) => {
            error!("Could not initialize database: {e}");
            return ExitCode::FAILURE;
        }
    };

    
    let command = args.command.unwrap_or(Command::Start { open_browser: true });
    let console = StandardStream::stderr(ColorChoice::Auto);
    if let Err(e) = perform_command(command, console, term_width, store) {
        error!("{e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
