//! Shared text: units, counts, times, dialogs, and the command-line help.

/// One command of the hint bar: a key and its effect. A note has an empty key.
pub type HintPart = (&'static str, &'static str);

/// The text between two parts of a summary line.
pub const SUMMARY_GAP: &str = " · ";

pub const CLIPBOARD_UNAVAILABLE: &str = "Clipboard unavailable. Paste with the terminal.";
pub const COPY_SENT_TO_TERMINAL: &str = "Copy request sent to the terminal. No confirmation.";
pub const COPY_FAILED: &str = "Copy failed.";

pub const FIND_LABEL: &str = "Find:    ";
pub const REPLACE_LABEL: &str = "Replace: ";
pub const NO_MATCHES: &str = "no matches";

pub fn pool_usage(busy: usize, limit: usize) -> String {
    format!("Pool {busy}/{limit}")
}

pub fn data_usage(used_mib: usize, limit_mib: usize) -> String {
    format!("Data {used_mib}/{limit_mib} MiB")
}

pub const SMALL_TERMINAL: &str = "Small terminal";
pub const MINIMUM_SIZE: &str = "Minimum size";

pub const NO_CONNECTION: &str =
    "No database. Use Ctrl+W h and n in the navigator, or a to add a connection.";

/// A size of data in the largest unit that keeps a value of 1 or more.
pub fn byte_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    // The value has one decimal, so 1023.96 shows as 1.0 of the next unit.
    while value >= 1023.95 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// A count of the connector as a count of the screen.
pub fn wide(number: u64) -> usize {
    usize::try_from(number).unwrap_or(usize::MAX)
}
pub const BUTTON_CANCEL: &str = "Cancel";
pub const TITLE_BROWSER: &str = "Choose a file";
pub const BUTTON_BROWSE: &str = "...";
pub const BROWSER_USE_FOLDER: &str = "Use this folder";
pub const BROWSER_EMPTY: &str = "No matching items";
pub const BROWSER_BAD_PATH: &str = "A text field cannot hold this path.";

pub fn browser_unreadable(reason: &str) -> String {
    format!("Cannot read the folder: {reason}")
}
pub const COPIED: &str = "Copied.";
pub const OPTION_CANCEL: &str = "Cancel";
pub const OPTION_CONTINUE: &str = "Run";
pub const OPTION_RETRY: &str = "Try again";
pub const OPTION_REMOVE: &str = "Remove";

pub fn count(number: usize) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

pub(super) fn plural(number: usize, noun: &str) -> String {
    let ending = if number == 1 { "" } else { "s" };
    format!("{} {noun}{ending}", count(number))
}

pub const HELP: &str = "\
sqln: terminal database client

Usage: sqln [options] [target]
       sqln mcp

Assistant:
  mcp                   Serve MCP over stdio for open SQLN instances. Select an
                         instance and approve access in its terminal interface.

Target:
  A SQLite file, a DuckDB target such as duckdb:file.duckdb or duckdb: (in
  memory), or a URI such as postgres://user@host/name. Without a password
  in the URI, SQLN asks for it. A password in a URI stays in the shell history.
  The connection of a target lasts for this session, and SQLN saves nothing of it.

Options:
  --project <folder>     Use the project in this folder. Without it, SQLN uses
                         the nearest .sqln folder above the working folder.
  --script <file>        Open a .sql file as a console at the start. It needs a
                         target or a saved connection.
  --execute <SQL>       Run SQL without the terminal interface. Several
                         instructions run in order. Use - to read the script
                         from standard input, up to 10 MiB.
  --output csv|json     Set the result format for --execute. Every result set
                         is written in order. Without it, standard output stays
                         empty and row counts go to the error stream.
  --connection <name>   Use a saved connection instead of a target.
  --scope global|project Select the scope of a saved connection name.
  --single-transaction  Run the whole script in one transaction that rolls
                         back on any failure. Redis and Kafka have no support.
  --continue-on-error   Keep running after a failed instruction and report
                         every failure. The exit code still marks a failure.
  --yes                 Authorize protected execution and database creation.
                         Without it, SQLN asks on the terminal. Trust stays separate.
  --password-file <file> Read a batch password. Otherwise use SQLN_PASSWORD,
                         then a saved credential, then a terminal question.
  --log <file>           Write a log of events, without SQL or values.
  -h, --help             Show this text.
  -V, --version          Show the version.

Development options:
  --dialect <name>       Use postgres, mysql, sqlite, duckdb, redis, or kafka in a
                         console without a connection.

The settings and the connections are in config.json, in the global folder
(~/.config/sqln, or $XDG_CONFIG_HOME/sqln) and in the .sqln folder of the project.
";
