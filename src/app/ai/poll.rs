//! The answers of AI runs: polling, proposals, and retries.

use super::*;

impl App {
    /// Shows a response in the editor: ghost text at its anchor for an insertion, an
    /// inline diff for a replacement. An explanation shows in a text window.
    pub(super) fn show_ai_proposal(&mut self, id: u64, completed: Completed) {
        let Some(run) = self.ai.runs.iter().find(|r| r.id == id) else {
            return;
        };
        let (console, target) = match run.kind {
            RunKind::Completion(ref target) => (run.console, target.clone()),
            RunKind::Explain(ref explanation) => {
                let label = explanation.label();
                self.show_ai_explanation(label, completed);
                return;
            }
        };
        let Some(index) = self.console_index(console) else {
            self.cancel_ai_id(id);
            return;
        };
        // The answer shows at the requested anchor. An edit of that line, and
        // only that, discards it; an edit elsewhere moves the anchor.
        let Some(row) = self.tabs.consoles[index].editor.ai_line_row(&target) else {
            self.cancel_ai_id(id);
            return;
        };
        let mut at = target.start;
        at.row = row;
        let keys = self.ghost_keys();
        self.tabs.consoles[index].editor.set_ghost_keys(keys);
        self.tabs.consoles[index]
            .editor
            .start_ghost(at, &completed.text);
    }

    pub(in crate::app) fn ai_retry(&mut self, id: u64) {
        if let Some(run) = self.ai.runs.iter_mut().find(|r| r.id == id) {
            run.sending = false;
        }
        self.continue_ai();
    }

    pub(in crate::app) fn ai_session(&mut self, id: u64) {
        if self.ai.runs.iter().any(|r| r.id == id) {
            self.ai.prompt = Some(PasswordPrompt::new(
                "AI session key",
                "Use only for this completion. Nothing is saved.".into(),
                false,
            ));
        }
    }

    pub(in crate::app) fn poll_ai(&mut self) -> bool {
        self.sync_ai();
        let mut changed = false;
        for _ in 0..8 {
            let Ok(event) = self.ai.schema_receiver.try_recv() else {
                break;
            };
            changed = true;
            match event {
                ai::schema::Event::Waiting(id, generation) => {
                    if let Some(run) = self
                        .ai
                        .runs
                        .iter_mut()
                        .find(|r| r.connection == id && r.generation == generation)
                    {
                        run.message = "Waiting for connection";
                    }
                }
                ai::schema::Event::Reading(id, generation) => {
                    if let Some(run) = self
                        .ai
                        .runs
                        .iter_mut()
                        .find(|r| r.connection == id && r.generation == generation)
                    {
                        run.message = "Reading schema";
                    }
                }
                ai::schema::Event::Loaded(id, generation, result) => {
                    if let Some(cache) = self
                        .ai
                        .caches
                        .get_mut(&id)
                        .filter(|c| c.generation == generation && !c.cancel.is_cancelled())
                    {
                        cache.loading = false;
                        match result {
                            Ok(snapshot) => {
                                cache.snapshot = Some(snapshot);
                                cache.warmed = true;
                            }
                            Err(error) => {
                                self.status = Some(error);
                                let dead: Vec<u64> = self
                                    .ai
                                    .runs
                                    .iter()
                                    .filter(|r| r.connection == id)
                                    .map(|r| r.id)
                                    .collect();
                                for run in dead {
                                    self.cancel_ai_id(run);
                                }
                            }
                        }
                    }
                }
            }
        }
        for _ in 0..8 {
            let Ok(work) = self.ai.receiver.try_recv() else {
                break;
            };
            changed = true;
            match work {
                Work::Consent(id, result) if self.ai.runs.iter().any(|r| r.id == id) => {
                    match result {
                        Ok(approved) => self.ai_consent_ready(id, approved),
                        Err(e) => {
                            self.cancel_ai();
                            self.status = Some(e);
                        }
                    }
                }
                Work::Approved(id, result) if self.ai.runs.iter().any(|r| r.id == id) => {
                    match result {
                        Ok(()) => {
                            if let Some(run) = self.ai.runs.iter().find(|r| r.id == id) {
                                let destination = run.destination.clone();
                                self.ai.approved.push(destination);
                            }
                            self.ai_consent_ready(id, true);
                        }
                        Err(e) => {
                            self.cancel_ai_id(id);
                            self.status = Some(e);
                        }
                    }
                }
                Work::Key(id, result) if self.ai.runs.iter().any(|r| r.id == id) => match result {
                    Ok(key) => self.send_ai(id, key),
                    Err(e) => self.show_dialog(
                        "AI credential unavailable",
                        e,
                        vec![
                            ("Cancel", DialogAction::AiCancel(id)),
                            ("Retry", DialogAction::AiRetry(id)),
                            ("Session key", DialogAction::AiSession(id)),
                        ],
                    ),
                },
                Work::Done(id, result) if self.ai.runs.iter().any(|r| r.id == id) => match result {
                    Ok(completed) => {
                        if let Some(run) = self
                            .ai
                            .runs
                            .iter_mut()
                            .find(|run| run.id == id && matches!(run.kind, RunKind::Explain(_)))
                        {
                            run.completed = Some(completed);
                            run.message = "Answer ready";
                        } else {
                            self.show_ai_proposal(id, completed);
                            self.cancel_ai_id(id);
                        }
                    }
                    Err(e) => {
                        self.cancel_ai_id(id);
                        self.status = Some(e);
                    }
                },
                Work::Form(token, result) if token == self.ai.form_token => {
                    if let Some(form) = &mut self.modals.settings_form {
                        form.ai_busy = false;
                    }
                    match result {
                        Ok(()) => {
                            if let Some((preferences, scope, ..)) = self.ai.pending_save.take() {
                                self.write_settings(preferences, scope);
                            }
                        }
                        Err(e) => {
                            self.show_dialog(
                                "Cannot save the API key",
                                e,
                                vec![
                                    ("Cancel", DialogAction::Dismiss),
                                    ("Retry", DialogAction::AiFormRetry),
                                    ("Use in this session", DialogAction::AiFormSession),
                                ],
                            );
                        }
                    }
                }
                Work::Test(token, result) if token == self.ai.form_token => {
                    if let Some(form) = &mut self.modals.settings_form {
                        form.ai_busy = false;
                    }
                    match result {
                        Ok(()) => {
                            self.status = Some(
                                "The endpoint accepts the connection. This test sends no SQL or schema."
                                    .into(),
                            );
                        }
                        Err(e) => {
                            if let Some(form) = &mut self.modals.settings_form {
                                form.note = Some(e);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.continue_ai();
        self.show_ready_ai_explanation() || changed
    }
}
