use std::path::PathBuf;
use std::process::ExitCode;

use sqln::app::{Options, run};
use sqln::sql::Dialect;
use sqln::store::{self, Root, Store};
use sqln::ui::text::HELP;

enum Command {
    Run(Cli),
    Help,
    Version,
}

#[derive(Default)]
struct Cli {
    project: Option<PathBuf>,
    dialect: Option<Dialect>,
    log: Option<PathBuf>,
}

fn parse(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut args = args;
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
            "--dialect" => {
                cli.dialect = Some(match args.next().as_deref() {
                    Some("postgres") => Dialect::Postgres,
                    Some("mysql") => Dialect::MySql,
                    Some("sqlite") => Dialect::Sqlite,
                    Some("redis") => Dialect::Redis,
                    other => return Err(format!("Unknown dialect: {}", other.unwrap_or(""))),
                });
            }
            other => return Err(format!("Unknown option: {other}")),
        }
    }
    Ok(Command::Run(cli))
}

/// Finds the folders of the two scopes. Without a project, the options carry the
/// folder where the application can offer one.
fn options_of(cli: Cli) -> Result<Options, String> {
    let global = store::default_global_dir()
        .ok_or("SQLN cannot find the home folder. Set HOME or XDG_CONFIG_HOME.")?;
    let working = std::env::current_dir().map_err(|error| error.to_string())?;
    let located = store::locate_project(&working, cli.project.as_deref());
    let mut options = Options {
        store: Some(Store {
            global: Root::global(global),
            project: located.root.as_deref().map(Root::project),
        }),
        offer_project: located.offer,
        ..Options::default()
    };
    if let Some(dialect) = cli.dialect {
        options.dialect = dialect;
    }
    Ok(options)
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
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
