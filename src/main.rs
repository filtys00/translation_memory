// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod database;
mod providers;
mod cli;
mod web_server;

use std::{env, io::Write, path::PathBuf, process::ExitCode};

use clap::{ArgAction, Parser};
use log::{Level, LevelFilter, debug, error};
use rusqlite::Connection;
use termcolor::{ColorChoice, StandardStream};

use crate::{cli::{Command, perform_command, wrapped_writeln}, database::TranslationStore};

/// Returns the default file name of the database file.
fn default_database_path() -> String {
    let mut db_file_name = env::current_exe().ok()
        .and_then(|p| p.file_stem().and_then(|n| n.to_str()).map(|n| n.to_string()))
        .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string());
    db_file_name.push_str(".sqlite");
    db_file_name
}

#[derive(Parser)]
#[command(version)]
struct Args {
    /// How verbose the logging should be [possible repeats: 2]
    #[arg(global = true, short, long, action = ArgAction::Count)]
    verbose: u8,

    /// Where the translations database file is located.
    #[arg(global = true, long, value_name = "PATH", default_value = default_database_path())]
    database_file: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let term_width = crossterm::terminal::size().map_or(80, |(cols, _)| cols as usize);
    let mut logger = env_logger::builder();
    logger.format(move |buf, record| {
        let level_style = buf.default_level_style(record.level());
        level_style.write_to(buf)?;
        write!(buf, "{}: ", record.level().as_str().to_ascii_lowercase())?;
        level_style.write_reset_to(buf)?;

        let indent = match record.level() {
            Level::Error | Level::Debug | Level::Trace => 5 + 2,
            Level::Warn | Level::Info => 4 + 2,
        };

        wrapped_writeln(buf, &record.args().to_string(), term_width, indent)
    });
    match args.verbose { // Only filter module env!("CARGO_PKG_NAME") to prevent logs from dependencies.
        0 => { logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Info); }
        1 => { logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Debug); }
        _ => { logger.filter_module(env!("CARGO_PKG_NAME"), LevelFilter::max()); }
    }
    logger.init();

    debug!("Using database file located at {:?}", args.database_file);
    let connection = match Connection::open(args.database_file) {
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
