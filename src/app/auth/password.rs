//! Password prompts, password checks, and password deletion of the authorization flow.

use super::*;

impl App {
    pub(in crate::app) fn on_password_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(prompt) = &mut self.auth.prompt else {
            return;
        };
        let action = prompt.on_key(key, vim);
        self.password_action(action);
    }

    pub(in crate::app) fn on_password_mouse(&mut self, mouse: MouseEvent) {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        let field = self
            .layout
            .password
            .iter()
            .find(|(_, area)| area.contains((mouse.column, mouse.row).into()))
            .map(|(field, _)| *field);
        if let Some(field) = field
            && let Some(prompt) = &mut self.auth.prompt
        {
            let action = prompt.click(field);
            self.password_action(action);
        }
    }

    pub(super) fn password_action(&mut self, action: PasswordAction) {
        match action {
            PasswordAction::None => {}
            PasswordAction::Cancel => self.cancel_active_auth(),
            PasswordAction::Submit { password, save } => {
                if self
                    .auth
                    .active
                    .as_ref()
                    .is_none_or(|attempt| !self.auth_current(attempt))
                {
                    self.cancel_active_auth();
                    return;
                }
                self.auth.prompt = None;
                let Some(attempt) = self.auth.active.as_mut() else {
                    return;
                };
                attempt.target.set_password(Some(password));
                attempt.save_reference = save;
                if attempt.target.refused {
                    self.verify_password();
                } else {
                    self.keep_password();
                }
            }
        }
    }

    pub(super) fn keep_password(&mut self) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        match (attempt.save_reference, attempt.no_vault) {
            (true, true) => self.auth_step(Step::WritePlain),
            (true, false) => self.auth_step(Step::Write),
            (false, _) => self.cache_auth_session(),
        }
    }

    pub(super) fn is_refused(&self, id: ConnectionId) -> bool {
        let generation = self.connection(id).map(|connection| connection.generation);
        self.auth
            .refused
            .get(&id)
            .is_some_and(|item| Some(item.generation) == generation)
    }

    /// One wrong password can fail several sessions, so only the first event counts.
    pub(in crate::app) fn on_password_refused(&mut self, id: ConnectionId, pool: u64) {
        let current = self
            .manager
            .as_ref()
            .and_then(|manager| manager.pool_serial(id))
            == Some(pool);
        if !current || self.is_refused(id) {
            return;
        }
        let Some(connection) = self.connection(id) else {
            return;
        };
        let generation = connection.generation;
        let identity = connection
            .key
            .as_ref()
            .zip(connection.definition.as_ref())
            .and_then(|(key, definition)| Identity::of(key, definition));
        let Some(identity) = identity else {
            return;
        };
        self.auth.ready.remove(&id);
        self.auth.refused.insert(
            id,
            Refusal {
                generation,
                count: 1,
            },
        );
        let credentials = self.auth.credentials.clone();
        if let Some(manager) = &self.manager {
            manager.spawn_blocking(move || {
                if let Ok(mut credentials) = credentials.lock() {
                    credentials.forget_session(&identity);
                }
            });
        }
        self.authorize(id, Use::NewPassword);
    }

    /// Opens the password request after a refusal. It returns false without a refusal.
    pub(in crate::app) fn ask_new_password(&mut self, id: ConnectionId) -> bool {
        let refused = self.is_refused(id);
        if refused {
            self.authorize(id, Use::NewPassword);
        }
        refused
    }

    /// A refused password never reaches a store, so the check comes before the save.
    pub(super) fn verify_password(&mut self) {
        let test = self.counters.next_test;
        self.counters.next_test += 1;
        let Some(attempt) = &mut self.auth.active else {
            return;
        };
        attempt.step = Step::Verify;
        attempt.verify = Some(test);
        let definition = attempt.target.definition.clone();
        if let Some(manager) = &self.manager {
            manager.test_connection(
                test,
                crate::connector::open(&definition),
                definition.connect_timeout,
                false,
            );
        }
    }

    /// Takes the answer of `verify_password`. It returns false for a test of the form.
    pub(in crate::app) fn on_password_verified<T>(
        &mut self,
        test: u64,
        result: &Result<T, ConnectError>,
    ) -> bool {
        let Some(attempt) = self
            .auth
            .active
            .as_mut()
            .filter(|item| item.verify == Some(test))
        else {
            return false;
        };
        attempt.verify = None;
        let id = attempt.target.connection;
        match result {
            Ok(_) => {
                attempt.target.refused = false;
                if let Some(id) = id {
                    self.auth.refused.remove(&id);
                }
                self.keep_password();
            }
            Err(error) if error.password_refused => {
                if let Some(refusal) = id.and_then(|id| self.auth.refused.get_mut(&id)) {
                    refusal.count += 1;
                }
                self.cancel_active_auth();
                self.show_dialog(
                    text::TITLE_PASSWORD_REFUSED,
                    text::AUTH_SUSPENDED.to_owned(),
                    vec![(text::OPTION_CLOSE, DialogAction::Dismiss)],
                );
            }
            Err(error) => {
                let message = error.message.clone();
                self.cancel_active_auth();
                self.status = Some(message);
            }
        }
        true
    }

    /// Deletes the saved password of a connection after its own confirmation.
    pub(in crate::app) fn delete_password(&mut self, identity: Identity) {
        let credentials = self.auth.credentials.clone();
        let sender = self.auth.sender.clone();
        let Some(manager) = &self.manager else {
            return;
        };
        manager.spawn_blocking(move || {
            let result = credentials
                .lock()
                .map_err(|_| VaultError::Failed)
                .and_then(|mut credentials| credentials.delete_saved(&identity));
            let _ = sender.send((DELETION, Reply::Deleted(result)));
        });
    }
}
