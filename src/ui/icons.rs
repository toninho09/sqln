//! The symbols of the interface. Each character fills one terminal column.

use crate::catalog::Group;
use crate::sql::Dialect;
use crate::store::config::ConnectionIcon;

/// One set of symbols. An empty symbol means that the set has no icon for the item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icons {
    pub run: &'static str,
    pub script: &'static str,
    pub stop: &'static str,
    pub mode: &'static str,
    pub commit: &'static str,
    pub rollback: &'static str,
    pub apply: &'static str,
    pub clear: &'static str,
    pub load_more: &'static str,
    pub previous_page: &'static str,
    pub next_page: &'static str,
    pub schema: &'static str,
    pub table: &'static str,
    pub view: &'static str,
    pub column: &'static str,
    pub routine: &'static str,
    pub sequence: &'static str,
    pub trigger: &'static str,
    pub data_type: &'static str,
    pub console: &'static str,
    /// The mark at the start of the active tab.
    pub tab_mark: &'static str,
    /// The mark at the start of the form row with the focus.
    pub focus_mark: &'static str,
    /// The mark that closes a main tab.
    pub close: &'static str,
    /// The frames of the mark of a tab with a run in progress.
    pub running: [&'static str; 4],
    pub done: &'static str,
    pub failed: &'static str,
    pub stopped: &'static str,
    pub connected: &'static str,
    pub disconnected: &'static str,
    /// The symbol before the name of a connection in a set without logos.
    pub database: &'static str,
    /// True when the symbol of an engine is a logo. The logo stands before the name.
    /// In a set without logos, that symbol is a short text after the name.
    pub logos: bool,
    pub postgres: &'static str,
    pub mysql: &'static str,
    pub sqlite: &'static str,
    pub duckdb: &'static str,
    pub redis: &'static str,
    pub kafka: &'static str,
    /// The marks of a key column in a diagram box. A column in both keys shows the first.
    pub primary_key: &'static str,
    pub foreign_key: &'static str,
    /// The ends of a line of a diagram, at the table that the key reads.
    pub arrow_left: &'static str,
    pub arrow_right: &'static str,
}

/// Returns the symbol of a connection icon. These symbols do not depend on the set of
/// icons, because the usual fonts of a terminal have them and a Nerd Font is optional.
/// Keep symbols with one column and with text presentation, not emoji presentation.
pub fn connection_symbol(icon: ConnectionIcon) -> &'static str {
    match icon {
        ConnectionIcon::Warning => "⚠",
        ConnectionIcon::Skull => "☠",
        ConnectionIcon::Radiation => "☢",
        ConnectionIcon::Flag => "⚑",
        ConnectionIcon::Crown => "♛",
        ConnectionIcon::Flask => "⚗",
        ConnectionIcon::Star => "★",
        ConnectionIcon::Diamond => "◆",
        ConnectionIcon::Tools => "⚒",
        ConnectionIcon::Gear => "⚙",
        ConnectionIcon::Home => "⌂",
        ConnectionIcon::Heart => "♥",
        ConnectionIcon::Snowflake => "❄",
        ConnectionIcon::Check => "✔",
    }
}

impl Icons {
    /// Returns the set of `NO_COLOR`. The text alone identifies each item.
    pub const fn plain() -> Self {
        Self {
            run: "",
            script: "",
            stop: "",
            mode: "",
            commit: "",
            rollback: "",
            apply: "",
            clear: "",
            load_more: "",
            previous_page: "",
            next_page: "",
            schema: "",
            table: "",
            view: "",
            column: "",
            routine: "",
            sequence: "",
            trigger: "",
            data_type: "",
            console: "",
            tab_mark: " ",
            focus_mark: ">",
            close: "X",
            running: ["|", "/", "-", "\\"],
            done: "ok",
            failed: "ERR",
            stopped: "stop",
            connected: "●",
            disconnected: "",
            database: "",
            logos: false,
            postgres: "pg",
            mysql: "my",
            sqlite: "sq",
            duckdb: "dk",
            redis: "rd",
            kafka: "kf",
            primary_key: "PK",
            foreign_key: "FK",
            arrow_left: "<",
            arrow_right: ">",
        }
    }

    /// Returns the default set. The symbols work with the usual terminal fonts.
    pub const fn unicode() -> Self {
        Self {
            run: "▶",
            script: "▶▶",
            stop: "■",
            mode: "◐",
            commit: "✓",
            rollback: "↩",
            apply: "✓",
            clear: "✕",
            load_more: "↓",
            previous_page: "◀",
            next_page: "▶",
            schema: "◇",
            table: "▦",
            view: "◫",
            column: "·",
            routine: "ƒ",
            sequence: "#",
            trigger: "↯",
            data_type: "◈",
            console: "",
            tab_mark: "▎",
            focus_mark: "▌",
            close: "×",
            running: ["◐", "◓", "◑", "◒"],
            done: "✓",
            failed: "✗",
            stopped: "■",
            connected: "●",
            disconnected: "○",
            database: "⛁",
            logos: false,
            postgres: "pg",
            mysql: "my",
            sqlite: "sq",
            duckdb: "dk",
            redis: "rd",
            kafka: "kf",
            primary_key: "◆",
            foreign_key: "◇",
            arrow_left: "◀",
            arrow_right: "▶",
        }
    }

    /// Returns the set for a Nerd Font. The private code points need that font.
    pub const fn nerd() -> Self {
        Self {
            run: "\u{f04b}",
            script: "\u{f050}",
            stop: "\u{f04d}",
            mode: "\u{f021}",
            commit: "\u{f00c}",
            rollback: "\u{f0e2}",
            apply: "\u{f00c}",
            clear: "\u{f00d}",
            load_more: "\u{f078}",
            previous_page: "\u{f053}",
            next_page: "\u{f054}",
            schema: "\u{f07b}",
            table: "\u{f0ce}",
            view: "\u{f06e}",
            column: "\u{f0db}",
            routine: "\u{f121}",
            sequence: "\u{f162}",
            trigger: "\u{f0e7}",
            data_type: "\u{f02b}",
            console: "\u{f120}",
            tab_mark: "▎",
            focus_mark: "▌",
            close: "\u{f00d}",
            running: ["◐", "◓", "◑", "◒"],
            done: "\u{f00c}",
            failed: "\u{f00d}",
            stopped: "\u{f04d}",
            connected: "●",
            disconnected: "○",
            database: "",
            logos: true,
            postgres: "\u{e76e}",
            mysql: "\u{e704}",
            sqlite: "\u{e7c4}",
            duckdb: "\u{e7c4}",
            redis: "\u{e76d}",
            // Nerd Fonts has no Kafka logo in the dev set, so the database symbol stands in.
            kafka: "\u{f1c0}",
            primary_key: "\u{f084}",
            foreign_key: "\u{f0c1}",
            arrow_left: "◀",
            arrow_right: "▶",
        }
    }

    /// Returns the symbol of an object of a group.
    pub fn object(&self, group: Group) -> &'static str {
        match group {
            Group::Routines => self.routine,
            Group::Sequences => self.sequence,
            Group::Triggers => self.trigger,
            Group::Types => self.data_type,
        }
    }

    /// Returns the mark of a database engine.
    pub fn engine(&self, dialect: Dialect) -> &'static str {
        match dialect {
            Dialect::Postgres => self.postgres,
            Dialect::MySql => self.mysql,
            Dialect::Sqlite => self.sqlite,
            Dialect::DuckDb => self.duckdb,
            Dialect::Redis => self.redis,
            Dialect::Kafka => self.kafka,
        }
    }

    /// Returns each symbol of the set, for the checks of the column width.
    pub fn all(&self) -> [&'static str; 43] {
        [
            self.run,
            self.script,
            self.stop,
            self.mode,
            self.commit,
            self.rollback,
            self.apply,
            self.clear,
            self.load_more,
            self.previous_page,
            self.next_page,
            self.schema,
            self.table,
            self.view,
            self.column,
            self.routine,
            self.sequence,
            self.trigger,
            self.data_type,
            self.console,
            self.tab_mark,
            self.focus_mark,
            self.close,
            self.running[0],
            self.running[1],
            self.running[2],
            self.running[3],
            self.done,
            self.failed,
            self.stopped,
            self.connected,
            self.disconnected,
            self.database,
            self.postgres,
            self.mysql,
            self.sqlite,
            self.duckdb,
            self.redis,
            self.kafka,
            self.primary_key,
            self.foreign_key,
            self.arrow_left,
            self.arrow_right,
        ]
    }
}
