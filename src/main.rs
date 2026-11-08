use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::PathBuf;
use std::process::ExitCode;

use sqln::app::{Options, run};
use sqln::connection::{Definition, Source};
use sqln::connector::duckdb::{DuckdbTarget, file_source as duckdb_source};
use sqln::connector::target::{OpenTarget, open_target};
use sqln::sql::Dialect;
use sqln::store::{self, Root, Store};
use sqln::terminal::read_password;
use sqln::ui::text::{self, HELP};

enum Command {
    Mcp,
    Run(Cli),
    Help,
    Version,
}

#[derive(Default)]
struct Cli {
    project: Option<PathBuf>,
    dialect: Option<Dialect>,
    log: Option<PathBuf>,
    /// A `.sql` file that opens as a console at the start.
    script: Option<PathBuf>,
    /// The file or the URI to open.
    target: Option<String>,
}

fn parse(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut args = args.peekable();
    if args.peek().is_some_and(|arg| arg == "mcp") {
        args.next();
        let next = args.next();
        return if next.is_none() {
            Ok(Command::Mcp)
        } else if matches!(next.as_deref(), Some("--help" | "-h")) && args.next().is_none() {
            Ok(Command::Help)
        } else {
            Err("sqln mcp takes no arguments. Select the instance with list_instances.".into())
        };
    }
    let mut cli = Cli::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--project" => {
                let folder = args.next().ok_or("--project needs a folder.")?;
                cli.project = Some(PathBuf::from(folder));
            }
            "--log" => {
                let file = args.next().ok_or("--log needs a file.")?;
                cli.log = Some(PathBuf::from(file));
            }
            "--script" => {
                let file = args.next().ok_or("--script needs a .sql file.")?;
                cli.script = Some(PathBuf::from(file));
            }
            "--dialect" => {
                cli.dialect = Some(match args.next().as_deref() {
                    Some("postgres") => Dialect::Postgres,
                    Some("mysql") => Dialect::MySql,
                    Some("sqlite") => Dialect::Sqlite,
                    Some("duckdb") => Dialect::DuckDb,
                    Some("redis") => Dialect::Redis,
                    other => return Err(format!("Unknown dialect: {}", other.unwrap_or(""))),
                });
            }
            other if other.starts_with('-') => return Err(format!("Unknown option: {other}")),
            _ if cli.target.is_some() => return Err(text::ONE_TARGET.to_owned()),
            other => cli.target = Some(other.to_owned()),
        }
    }
    Ok(Command::Run(cli))
}

/// Adds the target of the command line. Its connection lasts for this session.
fn open(options: &mut Options, target: &str) -> Result<(), String> {
    match open_target(target)? {
        OpenTarget::File(path) => options.database = Some(path),
        OpenTarget::Duckdb(text) => {
            let target = DuckdbTarget {
                source: duckdb_source(&text),
                allow_create: true,
                read_only: false,
            };
            options.connections.push(Definition::duckdb(target));
        }
        OpenTarget::Network(mut target) => {
            if target.password.is_none() && target.engine.needs_user() && stdin().is_terminal() {
                target.password = read_password(&text::password_prompt(&target.user))
                    .map_err(|error| error.to_string())?;
            }
            let name = target.label();
            options
                .connections
                .push(Definition::new(name, Source::Network(target)));
        }
    }
    options.session_target = true;
    Ok(())
}

/// Finds the folders of the two scopes. Without a project, the options carry the
/// folder where the application can offer one.
fn options_of(cli: Cli) -> Result<Options, String> {
    let global = store::default_global_dir()
        .ok_or("SQLN cannot find the home folder. Set HOME or XDG_CONFIG_HOME.")?;
    let working = std::env::current_dir().map_err(|error| error.to_string())?;
    let located = store::locate_project(&working, cli.project.as_deref());
    let mut options = Options {
        agent_ipc: true,
        store: Some(Store {
            global: Root::global(global),
            project: located.root.as_deref().map(Root::project),
        }),
        offer_project: located.offer,
        start_home: true,
        ..Options::default()
    };
    if let Some(dialect) = cli.dialect {
        options.dialect = dialect;
    }
    if let Some(script) = &cli.script {
        if !script.is_file() {
            return Err(format!(
                "--script needs an existing file: {}",
                script.display()
            ));
        }
        // Without a target, the file opens in a saved connection. There is no other
        // connection to pick at the start.
        let saved = options
            .store
            .as_ref()
            .map(store::config::load)
            .and_then(|loaded| loaded.config.ok())
            .is_some_and(|config| !config.connections.is_empty());
        if cli.target.is_none() && !saved {
            return Err("--script needs a connection: give a target or save a connection.".into());
        }
        options.script = Some(script.clone());
    }
    if let Some(target) = &cli.target {
        open(&mut options, target)?;
    }
    Ok(options)
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Command::Mcp) => {
            if let Err(error) = sqln::mcp::run() {
                eprintln!("sqln: {error}");
                return ExitCode::FAILURE;
            }
        }
        Ok(Command::Help) => print!("{HELP}"),
        Ok(Command::Version) => println!("sqln {}", env!("CARGO_PKG_VERSION")),
        Ok(Command::Run(cli)) => {
            let log = cli.log.clone();
            let options = match options_of(cli) {
                Ok(options) => options,
                Err(message) => {
                    eprintln!("sqln: {message}");
                    return ExitCode::FAILURE;
                }
            };
            if let Some(file) = &log
                && let Err(error) = sqln::log::open(file)
            {
                eprintln!("sqln: {}: {error}", file.display());
                return ExitCode::FAILURE;
            }
            if let Err(error) = run(options) {
                // The write can fail when the terminal closed. A failed write must not
                // stop the exit with an error of the print.
                let _ = writeln!(stderr(), "sqln: {error}");
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
