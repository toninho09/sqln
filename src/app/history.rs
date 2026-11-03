//! The query history: the notes of each statement and the window that lists them.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::KeyEvent;

use super::App;
use super::scripts::age;
use crate::connection::Connection;
use crate::exec::{StatementKey, StatementStatus};
use crate::filter_list::ListRequest;
use crate::history::{HistoryRow, history_list};
use crate::sql::is_secret;
use crate::store::history::{self, Entry, LOAD_LIMIT, STATEMENT_LIMIT};
use crate::ui::text;

/// The lines that wait for their write.
#[derive(Default)]
pub(super) struct HistoryLog {
    pending: Vec<u8>,
    /// The connection and the text of the last entry. A repetition stays out.
    last: Option<(String, String)>,
    /// True after a failed write, so the warning shows once.
    failed: bool,
}

impl App {
    /// Returns true when the history works for this connection. A connection without
    /// a saved definition lasts for one session, so it leaves no notes.
    pub(super) fn history_is_open_to(&self, connection: Option<&Connection>) -> bool {
        connection.is_some_and(|item| item.key.is_some())
            && self
                .persist
                .as_ref()
                .is_some_and(|persist| persist.query_history)
    }

    /// Notes a statement that reached the database.
    pub(super) fn note_statement(&mut self, key: &StatementKey, status: &StatementStatus) {
        let ok = match status {
            StatementStatus::Success => true,
            StatementStatus::NotExecuted | StatementStatus::Running => return,
            _ => false,
        };
        let Some(console) = self
            .consoles
            .iter()
            .find(|item| item.id == key.console)
            .filter(|console| console.explain.is_none())
        else {
            return;
        };
        let connection = console.connection.and_then(|id| self.connection(id));
        let Some(sql) = console.statement_at(key) else {
            return;
        };
        if !self.history_is_open_to(connection) {
            return;
        }
        let Some(connection) = connection else { return };
        let Some(id) = connection.key.as_ref().map(|key| key.id.clone()) else {
            return;
        };
        if sql.len() > STATEMENT_LIMIT || is_secret(sql, connection.dialect) {
            return;
        }
        let sql = sql.to_owned();
        let Some(persist) = &mut self.persist else {
            return;
        };
        let log = &mut persist.history;
        let same = |(connection, last): &(String, String)| *connection == id && *last == sql;
        if log.last.as_ref().is_some_and(same) {
            return;
        }
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_secs());
        let entry = Entry {
            at,
            connection: id,
            ok,
            sql,
        };
        history::push_line(&mut log.pending, &entry);
        log.last = Some((entry.connection, entry.sql));
    }

    /// Writes the notes of this frame. A failed write never stops a run.
    pub(super) fn flush_history(&mut self) {
        let Some(persist) = &mut self.persist else {
            return;
        };
        if persist.history.pending.is_empty() {
            return;
        }
        let lines = std::mem::take(&mut persist.history.pending);
        if let Err(error) = history::append(&persist.store.global, &lines)
            && !std::mem::replace(&mut persist.history.failed, true)
        {
            self.status = Some(text::history_failed(&error.to_string()));
        }
    }

    pub(super) fn open_history(&mut self) {
        let connection = self.active_connection();
        if self.table_active.is_some() || !self.history_is_open_to(connection) {
            return;
        }
        let Some(id) = connection.and_then(|item| item.key.as_ref()) else {
            return;
        };
        let Some(persist) = &self.persist else {
            return;
        };
        let entries = history::load(&persist.store.global, &id.id, LOAD_LIMIT);
        if entries.is_empty() {
            self.status = Some(text::NO_HISTORY.to_owned());
            return;
        }
        let rows = entries.into_iter().map(|entry| {
            let time = UNIX_EPOCH + Duration::from_secs(entry.at);
            HistoryRow::new(entry.ok, age(Some(time)), entry.sql)
        });
        self.console_mut().editor.close_suggestions();
        self.history = Some(history_list(rows.collect()));
    }

    pub(super) fn on_history_key(&mut self, key: KeyEvent) {
        let Some(list) = &mut self.history else {
            return;
        };
        let request = list.on_key(key);
        self.on_history_request(request);
    }

    /// Inserts the chosen statement at the cursor. It does not run the statement.
    pub(super) fn on_history_request(&mut self, request: ListRequest) {
        match request {
            ListRequest::None => {}
            ListRequest::Cancel => self.history = None,
            ListRequest::Choose(index) => {
                let list = self.history.take();
                let row = list.as_ref().and_then(|list| list.row(index));
                if let Some(row) = row {
                    self.leave_navigator();
                    self.on_paste(&row.sql);
                }
            }
        }
    }
}
