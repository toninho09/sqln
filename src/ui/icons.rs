//! The symbols of the interface. Each character fills one terminal column.

use crate::sql::Dialect;

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
    pub postgres: &'static str,
    pub mysql: &'static str,
    pub sqlite: &'static str,
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
            postgres: "pg",
            mysql: "my",
            sqlite: "sq",
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
            postgres: "pg",
            mysql: "my",
            sqlite: "sq",
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
            postgres: "\u{e76e}",
            mysql: "\u{e704}",
            sqlite: "\u{e7c4}",
        }
    }

    /// Returns the mark of a database engine.
    pub fn engine(&self, dialect: Dialect) -> &'static str {
        match dialect {
            Dialect::Postgres => self.postgres,
            Dialect::MySql => self.mysql,
            Dialect::Sqlite => self.sqlite,
        }
    }

    /// Returns each symbol of the set, for the checks of the column width.
    pub fn all(&self) -> [&'static str; 31] {
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
            self.postgres,
            self.mysql,
            self.sqlite,
        ]
    }
}
