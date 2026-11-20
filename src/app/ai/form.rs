//! The AI settings form: sessions, saves, and the connection test.

use super::*;

impl App {
    /// Closes the settings and stops a vault write or a test of the provider.
    pub(in crate::app) fn close_settings(&mut self) {
        self.modals.settings_form = None;
        self.ai.form_token += 1;
        self.ai.form_cancel.cancel();
        self.ai.pending_save = None;
    }

    /// The vault write failed. The save starts again from the current fields.
    pub(in crate::app) fn ai_form_retry(&mut self) {
        self.ai.pending_save = None;
        self.save_settings();
    }

    pub(in crate::app) fn ai_form_session(&mut self) {
        let Some((preferences, scope, key, account)) = self.ai.pending_save.take() else {
            return;
        };
        self.ai.session_keys.insert(account, key);
        if let Some(ai) = preferences.ai_completion.clone()
            && let Some(form) = &mut self.modals.settings_form
        {
            form.set_ai_preferences(ai);
        }
        self.write_settings(preferences, scope);
        self.status = Some("The API key stays in this session only.".into());
    }

    /// Saves the settings. A typed vault key goes to the vault first; the save
    /// continues when the write ends.
    pub(in crate::app) fn save_settings(&mut self) {
        // The vault write runs on the manager, which starts at its first use.
        let writes_key = self
            .modals
            .settings_form
            .as_ref()
            .is_some_and(|form| !form.ai_busy && form.ai_new_key().is_some());
        if writes_key && !self.ensure_manager() {
            return;
        }
        let Some(form) = &mut self.modals.settings_form else {
            return;
        };
        if form.ai_busy {
            return;
        }
        let preferences = match form.preferences() {
            Ok(preferences) => preferences,
            Err(message) => {
                form.note = Some(message);
                return;
            }
        };
        let scope = form.scope;
        let Some(key) = form.ai_new_key() else {
            self.write_settings(preferences, scope);
            return;
        };
        let provider = preferences
            .ai_completion
            .as_ref()
            .and_then(|p| p.provider.clone())
            .ok_or_else(|| "Configure a provider.".to_owned());
        let provider = match provider {
            Ok(provider) => provider,
            Err(e) => {
                form.note = Some(e);
                return;
            }
        };
        let Credential::Vault { id, endpoint } = &provider.credential else {
            return;
        };
        form.ai_busy = true;
        form.note = None;
        self.ai.form_token += 1;
        let token = self.ai.form_token;
        self.ai.form_cancel.cancel();
        self.ai.form_cancel = CancellationToken::new();
        let cancel = self.ai.form_cancel.clone();
        let vault = self.ai.vault.clone();
        let sender = self.ai.sender.clone();
        let account = self.ai_account(scope, id, endpoint);
        self.ai.pending_save = Some((
            preferences,
            scope,
            Secret::new(key.clone()),
            account.clone(),
        ));
        if let Some(manager) = &self.manager {
            manager.spawn_blocking(move || {
                if cancel.is_cancelled() {
                    return;
                }
                let result = vault
                    .write_api_key(&account, &Secret::new(key))
                    .map_err(|e| format!("{e} The draft is not saved. Retry or use a key file."));
                let _ = sender.send(Work::Form(token, result));
            });
        }
    }

    /// The vault account of one scope and credential endpoint.
    pub(super) fn ai_account(&self, scope: Scope, id: &str, endpoint: &str) -> String {
        let root = self.persist_root(scope);
        let base = root.as_ref().map(|r| r.base.clone()).unwrap_or_default();
        let scope_name = self.ai_scope_name(scope, &base);
        ai::credentials::account(&scope_name, id, endpoint)
    }

    /// The vault scope name of one scope.
    pub(super) fn ai_scope_name(&self, scope: Scope, base: &std::path::Path) -> String {
        if scope == Scope::Project {
            match base.canonicalize() {
                Ok(p) => format!("project:{}", p.display()),
                Err(_) => "invalid-project".into(),
            }
        } else {
            "global".into()
        }
    }

    /// Tests the endpoint of the autocomplete provider from the settings.
    pub(in crate::app) fn test_ai_connection(&mut self) {
        // The test runs on the manager, which starts at its first use.
        let idle = (self.modals.settings_form.as_ref()).is_some_and(|form| !form.ai_busy);
        if idle && !self.ensure_manager() {
            return;
        }
        let Some(form) = &mut self.modals.settings_form else {
            return;
        };
        if form.ai_busy {
            return;
        }
        let provider = match form.ai_test_provider() {
            Ok(provider) => provider,
            Err(e) => {
                form.note = Some(e);
                return;
            }
        };
        let key = (!form.ai_key_text().is_empty()).then(|| Secret::new(form.ai_key_text()));
        form.ai_busy = true;
        form.note = None;
        self.ai.form_token += 1;
        let token = self.ai.form_token;
        self.ai.form_cancel.cancel();
        self.ai.form_cancel = CancellationToken::new();
        let cancel = self.ai.form_cancel.clone();
        let vault = self.ai.vault.clone();
        let sender = self.ai.sender.clone();
        let scope = form.scope;
        let root = self.persist_root(scope);
        let base = root.as_ref().map(|r| r.base.clone()).unwrap_or_default();
        let scope_name = self.ai_scope_name(scope, &base);
        if let Some(manager) = &self.manager {
            manager.spawn_ai(async move {
                let resolved = tokio::task::spawn_blocking(move || {
                    if key.is_some() {
                        Ok(key)
                    } else {
                        ai::credentials::read(&provider, &scope_name, &base, &vault)
                    }
                    .map(|key| (provider, key))
                })
                .await
                .map_err(|_| "Cannot read the API credential.".to_owned())
                .and_then(|r| r);
                let result = match resolved {
                    Ok((provider, key)) => ai::client::test(&provider, key.as_ref(), &cancel).await,
                    Err(e) => Err(e),
                };
                let _ = sender.send(Work::Test(token, result));
            });
        }
    }
}
