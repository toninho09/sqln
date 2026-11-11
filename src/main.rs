use std::io::{IsTerminal, Write, stderr, stdin};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sqln::app::{Options, run};
use sqln::connection::{Definition, Source};
use sqln::connector::duckdb::{DuckdbTarget, file_source as duckdb_source};
use sqln::connector::target::{OpenTarget, open_target};
use sqln::export::ExportFormat;
use sqln::sql::Dialect;
use sqln::store::Scope;
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
    execute: Option<String>,
    output: Option<ExportFormat>,
    connection: Option<String>,
    scope: Option<Scope>,
    single_transaction: bool,
    continue_on_error: bool,
    yes: bool,
    password_file: Option<PathBuf>,
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
            "--execute" => {
                if cli
                    .execute
                    .replace(args.next().ok_or("--execute needs SQL.")?)
                    .is_some()
                {
                    return Err("Use --execute only once.".into());
                }
            }
            "--output" => {
                let format = match args.next().as_deref() {
                    Some("csv") => ExportFormat::Csv,
                    Some("json") => ExportFormat::Json,
                    _ => return Err("--output needs csv or json.".into()),
                };
                if cli.output.replace(format).is_some() {
                    return Err("Use --output only once.".into());
                }
            }
            "--connection" => {
                if cli
                    .connection
                    .replace(args.next().ok_or("--connection needs a name.")?)
                    .is_some()
                {
                    return Err("Use --connection only once.".into());
                }
            }
            "--scope" => {
                let scope = match args.next().as_deref() {
                    Some("global") => Scope::Global,
                    Some("project") => Scope::Project,
                    _ => return Err("--scope needs global or project.".into()),
                };
                if cli.scope.replace(scope).is_some() {
                    return Err("Use --scope only once.".into());
                }
            }
            "--yes" => cli.yes = true,
            "--single-transaction" => cli.single_transaction = true,
            "--continue-on-error" => cli.continue_on_error = true,
            "--password-file" => {
                let path = PathBuf::from(args.next().ok_or("--password-file needs a file.")?);
                if cli.password_file.replace(path).is_some() {
                    return Err("Use --password-file only once.".into());
                }
            }
            "--dialect" => {
                cli.dialect = Some(match args.next().as_deref() {
                    Some("postgres") => Dialect::Postgres,
                    Some("mysql") => Dialect::MySql,
                    Some("sqlite") => Dialect::Sqlite,
                    Some("duckdb") => Dialect::DuckDb,
                    Some("redis") => Dialect::Redis,
                    Some("kafka") => Dialect::Kafka,
                    other => return Err(format!("Unknown dialect: {}", other.unwrap_or(""))),
                });
            }
            other if other.starts_with('-') => return Err(format!("Unknown option: {other}")),
            _ if cli.target.is_some() => return Err(text::ONE_TARGET.to_owned()),
            other => cli.target = Some(other.to_owned()),
        }
    }
    if cli.execute.is_some() {
        if cli.script.is_some() || cli.dialect.is_some() {
            return Err("--execute cannot be combined with --script or --dialect.".into());
        }
        if cli.target.is_some() == cli.connection.is_some() {
            return Err("--execute needs exactly one target or --connection <name>.".into());
        }
        if cli.scope.is_some() && cli.connection.is_none() {
            return Err("--scope needs --connection.".into());
        }
    } else if cli.output.is_some()
        || cli.connection.is_some()
        || cli.scope.is_some()
        || cli.yes
        || cli.password_file.is_some()
        || cli.single_transaction
        || cli.continue_on_error
    {
        return Err(
            "--output, --connection, --scope, --yes, --password-file, --single-transaction, and --continue-on-error need --execute."
                .into(),
        );
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
fn store_of(project: Option<&Path>) -> Result<(Store, Option<PathBuf>), String> {
    let global = store::default_global_dir()
        .ok_or("SQLN cannot find the home folder. Set HOME or XDG_CONFIG_HOME.")?;
    let working = std::env::current_dir().map_err(|error| error.to_string())?;
    let located = store::locate_project(&working, project);
    Ok((
        Store {
            global: Root::global(global),
            project: located.root.as_deref().map(Root::project),
        },
        located.offer,
    ))
}

fn options_of(cli: Cli) -> Result<Options, String> {
    let (store, offer) = store_of(cli.project.as_deref())?;
    let mut options = Options {
        agent_ipc: true,
        store: Some(store),
        offer_project: offer,
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
        Ok(Command::Run(cli)) if cli.execute.is_some() => {
            let store = match store_of(cli.project.as_deref()) {
                Ok((store, _)) => store,
                Err(message) => {
                    sqln::batch::report_error(&message);
                    return ExitCode::FAILURE;
                }
            };
            if let Some(file) = &cli.log
                && let Err(error) = sqln::log::open(file)
            {
                sqln::batch::report_error(&error.to_string());
                return ExitCode::FAILURE;
            }
            // The dash reads the script before SQLN resolves a connection, so no
            // password question comes before an input failure.
            let sql = match cli.execute.as_deref() {
                Some("-") => match sqln::batch::read_standard_input() {
                    Ok(text) => text,
                    Err(error) => {
                        sqln::batch::report_error(&error.message);
                        return ExitCode::from(if error.usage { 2 } else { 1 });
                    }
                },
                Some(sql) => sql.to_owned(),
                _ => unreachable!("The match checks --execute."),
            };
            let options = sqln::batch::Options {
                store,
                target: cli.target,
                connection: cli.connection,
                scope: cli.scope,
                sql,
                output: cli.output,
                single_transaction: cli.single_transaction,
                continue_on_error: cli.continue_on_error,
                yes: cli.yes,
                password_file: cli.password_file,
            };
            if let Err(error) = sqln::batch::run(options) {
                sqln::batch::report_error(&error.message);
                return ExitCode::from(if error.usage { 2 } else { 1 });
            }
        }
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
            eprintln!("sqln: {}\n\n{HELP}", sqln::grid::printable(&message));
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
