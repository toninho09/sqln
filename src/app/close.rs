//! Execution signals, and the questions of a close and of the application exit.

use super::{App, DialogAction};
use crate::connector::{TransactionCommand, TransactionState};
use crate::console::{AfterTransaction, Signal};
use crate::exec::ConsoleId;
use crate::ui::text;

impl App {
    pub(super) fn on_signal(&mut self, id: ConsoleId, signal: Signal) {
        match signal {
            Signal::ExecutionFinished { schema_changed } => {
                if schema_changed
                    && let Some(connection) = self.console_by_id(id).and_then(|item| item.connection)
                {
                    self.reload_catalog(connection);
                }
                if self.console_by_id(id).is_some_and(|console| console.closing) {
                    self.close_step(id);
                }
            }
            Signal::ConfirmClose => {
                let name = self.console_name(id);
                self.show_dialog(
                    text::TITLE_NO_ANSWER,
                    text::console_question(&name, text::BODY_NO_ANSWER),
                    vec![
                        (text::OPTION_KEEP_WAITING, DialogAction::KeepWaiting(id)),
                        (text::OPTION_CLOSE_CONNECTION, DialogAction::CloseConnection(id)),
                    ],
                );
            }
            Signal::TransactionChanged => self.on_transaction_changed(id),
            Signal::TransactionFailed(message) => self.on_transaction_failed(id, &message),
        }
    }

    fn console_name(&mut self, id: ConsoleId) -> String {
        self.console_by_id(id).map_or_else(String::new, |console| console.name.clone())
    }

    fn on_transaction_changed(&mut self, id: ConsoleId) {
        let Some(console) = self.console_by_id(id) else { return };
        if console.transaction != TransactionState::None {
            return;
        }
        let Some((_, after)) = console.pending_transaction.take() else { return };
        match after {
            AfterTransaction::Close => self.close_step(id),
            AfterTransaction::UseAutocommit => console.manual = false,
        }
    }

    /// The console stays open. A new attempt needs an explicit action on the same session.
    fn on_transaction_failed(&mut self, id: ConsoleId, message: &str) {
        let Some(console) = self.console_by_id(id) else { return };
        let Some((command, after)) = console.pending_transaction.take() else {
            self.status = Some(text::transaction_failed(message));
            return;
        };
        console.closing = false;
        self.show_dialog(
            text::TITLE_TRANSACTION,
            text::transaction_failed(message),
            vec![
                (text::OPTION_CANCEL, DialogAction::CancelClose(id)),
                (text::OPTION_RETRY, DialogAction::Transaction(id, command, after)),
            ],
        );
    }

    /// Asks the next question of a close. It runs again after each answer.
    pub(super) fn close_step(&mut self, id: ConsoleId) {
        let Some(console) = self.console_by_id(id) else { return };
        let name = console.name.clone();
        if console.is_busy() {
            if console.closing {
                return;
            }
            self.show_dialog(
                text::TITLE_RUNNING,
                text::console_question(&name, text::BODY_RUNNING),
                vec![
                    (text::OPTION_CANCEL, DialogAction::CancelClose(id)),
                    (text::OPTION_STOP_CLOSE, DialogAction::StopAndClose(id)),
                ],
            );
            return;
        }
        if console.in_transaction() {
            console.closing = false;
            let after = AfterTransaction::Close;
            let rollback = DialogAction::Transaction(id, TransactionCommand::Rollback, after);
            let commit = DialogAction::Transaction(id, TransactionCommand::Commit, after);
            self.show_dialog(
                text::TITLE_TRANSACTION,
                text::console_question(&name, text::BODY_TRANSACTION_CLOSE),
                vec![
                    (text::OPTION_CANCEL, DialogAction::CancelClose(id)),
                    (text::OPTION_ROLLBACK_CLOSE, rollback),
                    (text::OPTION_COMMIT_CLOSE, commit),
                ],
            );
            return;
        }
        console.closing = false;
        let Some(index) = self.console_index(id) else { return };
        if !self.flush_console(index) {
            return;
        }
        if self.exiting {
            self.exit_step();
        } else {
            self.remove_console(id);
        }
    }


    pub(super) fn remove_console(&mut self, id: ConsoleId) {
        let Some(index) = self.console_index(id) else { return };
        // The last tab ends the application. With a table open, a hidden console keeps
        // the rule that the application has one console, and no tab appears for it.
        let last = self.consoles.iter().filter(|console| !console.hidden).count() == 1;
        let replaced = last && !self.consoles[index].hidden;
        if replaced {
            if self.tables.is_empty() {
                self.quit = true;
                return;
            }
            self.keep_hidden_console();
        }
        self.save_editor_states(Some(index));
        let console = self.consoles.remove(index);
        for result in &console.results {
            self.budget.release(result.table.bytes);
        }
        if let Some(manager) = &mut self.manager {
            manager.close_console(console.id);
        }
        if self.active > index || self.active == self.consoles.len() {
            self.active = self.active.saturating_sub(1);
        }
        if replaced && self.table_active.is_none() {
            self.table_active = Some(0);
        }
    }

    /// Adds the empty console that stays out of the tab row, once.
    fn keep_hidden_console(&mut self) {
        if self.consoles.iter().any(|console| console.hidden) {
            return;
        }
        let dialect = self.fallback_dialect;
        let mut console = self.unbound_console(dialect);
        console.hidden = true;
        self.consoles.push(console);
    }

    /// The exit asks about each console in order, and it applies each answer at once.
    pub(super) fn request_exit(&mut self) {
        self.exiting = true;
        self.exit_step();
    }

    fn exit_step(&mut self) {
        let next = self.consoles.iter().position(|console| console.is_busy() || console.in_transaction());
        let Some(index) = next else {
            if self.flush_all() {
                self.quit = true;
            }
            return;
        };
        self.select_console(index);
        self.close_step(self.consoles[index].id);
    }

    pub(super) fn on_dialog_action(&mut self, action: DialogAction) {
        match action {
            DialogAction::Dismiss => {}
            DialogAction::CancelClose(id) => {
                self.exiting = false;
                if let Some(console) = self.console_by_id(id) {
                    console.closing = false;
                }
            }
            DialogAction::CreateDatabase => self.create_database(),
            DialogAction::CreateFormFile => self.create_form_file(),
            DialogAction::StopAndClose(id) => {
                if let Some(index) = self.console_index(id) {
                    self.consoles[index].closing = true;
                    self.stop(index);
                }
            }
            DialogAction::Transaction(id, command, after) => {
                let Some(console) = self.console_by_id(id) else { return };
                console.pending_transaction = Some((command, after));
                if after == AfterTransaction::Close {
                    console.closing = true;
                }
                if let Some(manager) = &self.manager {
                    manager.transaction(id, command);
                }
            }
            DialogAction::KeepWaiting(id) => self.confirm_close(id, false),
            DialogAction::CloseConnection(id) => self.confirm_close(id, true),
            DialogAction::RunAnyway(id, scope) => {
                if let Some(index) = self.console_index(id) {
                    self.consoles[index].transaction = TransactionState::None;
                    self.start_run(index, scope);
                }
            }
            DialogAction::RemoveConnection(connection) => self.remove_connection(connection),
            DialogAction::CreateProject => self.create_project(),
            DialogAction::ReloadConfig => self.reload_config(),
            DialogAction::IgnoreConfigChange => self.ignore_config_change(),
            DialogAction::DiscardAndClose(id) => {
                if let Some(console) = self.console_by_id(id) {
                    console.discard_text = true;
                }
                self.close_step(id);
            }
            DialogAction::DeleteScript(scope, id) => self.delete_script(scope, &id),
            DialogAction::Associate(console, connection) => self.associate(console, connection),
            DialogAction::DeletePassword(identity) => self.delete_password(identity),
            action @ (DialogAction::AuthCancel(_)
            | DialogAction::AuthConfirm(_)
            | DialogAction::AuthRetry(_)
            | DialogAction::AuthSession(_)
            | DialogAction::AuthPlain(_)
            | DialogAction::AuthMigrate(_)
            | DialogAction::AuthRemovePlain(_)) => self.auth_dialog(action),
        }
    }

    fn confirm_close(&self, id: ConsoleId, close: bool) {
        if let Some(manager) = &self.manager {
            manager.confirm_close(id, close);
        }
    }
}
