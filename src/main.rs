use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use sqln::app::{Options, run, sample_catalog};
use sqln::connection::{Definition, Source};
use sqln::connector::sqlite::SqliteTarget;
use sqln::connector::target::{Secret, parse_uri};
use sqln::editor::KeyboardMode;
use sqln::sql::Dialect;
use sqln::ui::text::HELP;

enum Command {
    Run(Box<Options>),
    Help,
    Version,
}

fn number(option: &str, value: Option<String>) -> Result<u64, String> {
    value
        .as_deref()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("{option} needs a number."))
}

fn parse(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut args = args;
    let mut options = Options { catalog: Some(sample_catalog()), ..Options::default() };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--vim" => options.keyboard = KeyboardMode::Vim,
            "--no-catalog" => options.catalog = None,
            "--read-only" => options.settings.read_only = true,
            "--sqlite" => {
                let path = PathBuf::from(args.next().ok_or("--sqlite needs a file.")?);
                if options.database.is_some() {
                    options.connections.push(Definition::sqlite(SqliteTarget::new(path)));
                } else {
                    options.database = Some(path);
                }
                options.catalog = None;
            }
            "--url" => {
                let uri = args.next().ok_or("--url needs a connection text.")?;
                let mut target = parse_uri(&uri)?;
                if target.password.is_none() {
                    target.password = std::env::var("SQLN_PASSWORD").ok().map(Secret::new);
                }
                let name = target.label();
                options.connections.push(Definition::new(name, Source::Network(target)));
                options.catalog = None;
            }
            "--row-cap" => {
                let rows = number("--row-cap", args.next())?;
                options.settings.row_cap = usize::try_from(rows).unwrap_or(usize::MAX);
            }
            "--sql-timeout" => {
                let seconds = number("--sql-timeout", args.next())?;
                options.settings.sql_timeout =
                    (seconds > 0).then(|| Duration::from_secs(seconds));
            }
            "--dialect" => {
                options.dialect = match args.next().as_deref() {
                    Some("postgres") => Dialect::Postgres,
                    Some("mysql") => Dialect::MySql,
                    Some("sqlite") => Dialect::Sqlite,
                    other => return Err(format!("Unknown dialect: {}", other.unwrap_or(""))),
                };
            }
            other => return Err(format!("Unknown option: {other}")),
        }
    }
    Ok(Command::Run(Box::new(options)))
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Command::Help) => print!("{HELP}"),
        Ok(Command::Version) => println!("sqln {}", env!("CARGO_PKG_VERSION")),
        Ok(Command::Run(options)) => {
            if let Err(error) = run(*options) {
                eprintln!("sqln: {error}");
                return ExitCode::FAILURE;
            }
        }
        Err(message) => {
            eprintln!("sqln: {message}\n\n{HELP}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
