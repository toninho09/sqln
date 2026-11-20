//! The start of an authorized use, its cancellation, and the dialogs of the authorization flow.

use super::*;

impl App {
    pub(super) fn dispatch_authorized(&mut self, action: Use, definition: Option<Definition>) {
        self.sync_command_history();
        let Some(manager) = &self.manager else { return };
        match action {
            Use::AiSchema(id, generation) => self.dispatch_ai_schema(id, generation),
            Use::Agent { job, .. } => self.dispatch_agent_job(&job),
            Use::Catalog(id, read) => {
                let schemas = self
                    .connection(id)
                    .and_then(|connection| connection.definition.as_ref())
                    .map(|definition| definition.schemas.clone())
                    .unwrap_or_default();
                manager.load_catalog(id, read, schemas);
                // The answer of an older read does not end the state, so it cannot start it.
                if let Some(connection) = self
                    .connection_mut(id)
                    .filter(|item| item.catalog_read == read)
                {
                    connection.catalog_state = CatalogState::Loading;
                }
            }
            Use::Columns(id, relation) => manager.load_columns(id, relation),
            Use::Run {
                console,
                connection,
                plan,
                options,
                revision,
                explain,
            } => {
                let Some(index) = self
                    .console_index(console)
                    .filter(|index| self.tabs.consoles[*index].connection == Some(connection))
                else {
                    return;
                };
                // An auto-refresh repeats the statements of this run, without a plan.
                if explain.is_none() {
                    self.tabs.consoles[index].auto_snapshot = Some(plan.statements.clone());
                }
                // The marks of the editor keep the text of the user.
                let statements = match &explain {
                    Some(explain) => plan
                        .statements
                        .iter()
                        .map(|sql| explain.wrap(sql))
                        .collect(),
                    None => plan.statements.clone(),
                };
                self.tabs.consoles[index]
                    .ran_parameters
                    .clone_from(&options.parameters);
                let execution = manager.run(console, statements, options);
                self.tabs.consoles[index].begin_execution(execution, &plan, &self.budget);
                self.tabs.consoles[index].explain = explain;
                if self.tabs.consoles[index].editor.revision() != revision {
                    self.tabs.consoles[index]
                        .editor
                        .set_marks_at_revision(&plan, revision);
                }
            }
            Use::Page {
                console,
                connection,
                request,
            } => {
                let Some(tab) = self
                    .tabs
                    .tables
                    .iter_mut()
                    .find(|tab| tab.id == console && tab.connection == connection)
                else {
                    return;
                };
                let options = RunOptions {
                    manual: false,
                    schema: None,
                    only_schema: false,
                    row_cap: Some(request.row_cap),
                    export: None,
                    stream: false,
                    continue_on_error: false,
                    hold_transaction: false,
                    untimed: false,
                    parameters: Vec::new(),
                    live: false,
                };
                let (first, show, kind) = (request.first, request.show, request.kind);
                let execution = manager.run(console, vec![request.sql], options);
                match kind {
                    PageKind::Rows => tab.started(execution, first, show),
                    kind => tab.started_side_request(execution, kind),
                }
            }
            Use::Diagram {
                console,
                connection,
                statements,
            } => self.start_diagram_read(console, connection, statements),
            Use::Compare {
                console,
                connection,
                statements,
                schema,
                untimed,
            } => self.start_compare_read(console, connection, statements, (schema, untimed)),
            Use::Definition {
                console,
                connection,
                statements,
                schema,
            } => self.start_definition_read(console, connection, statements, schema),
            Use::Export { console, read } => self.start_read(console, read),
            Use::Edit { console, job } => self.start_edit_job(console, job),
            Use::Import { console, step } => self.start_import_step(console, step),
            Use::Test { test, names } => {
                let Some(definition) = definition else { return };
                let (connector, limit) = (
                    crate::connector::open(&definition),
                    definition.connect_timeout,
                );
                manager.test_connection(test, connector, limit, names);
            }
            Use::Save => {}
            Use::NewPassword => self.status = Some(text::AUTH_PASSWORD_ACCEPTED.to_owned()),
        }
    }

    pub(super) fn cancel_auth_use(&mut self, action: &Use) {
        if let Use::AiSchema(id, generation) = action {
            self.cancel_ai_schema_auth(*id, *generation);
        }
        if let Some(console) = action.console() {
            self.cancel_agent_console(console);
        }
        if let Use::Agent { job, .. } = action {
            self.fail_agent_job(job, "Credential preparation was cancelled. Resolve it in SQLN and submit the operation again.");
        }
        if matches!(action, Use::Edit { .. }) {
            self.exiting = false;
        }
        if let Some(id) = action.console() {
            self.clear_wanted_import(id);
            let budget = self.budget.clone();
            if let Some(console) = self.console_by_id(id) {
                console.phase = Phase::Idle;
                if matches!(action, Use::Run { .. }) {
                    console.cancel_comparison("credential preparation was canceled", &budget);
                }
            }
            if let Some(tab) = self.tabs.tables.iter_mut().find(|tab| tab.id == id) {
                tab.phase = Phase::Idle;
                tab.edit_after = None;
                tab.edit_close = false;
                tab.edit_restore = None;
            }
            if let Some(tab) = self.tabs.diagrams.iter_mut().find(|tab| tab.id == id) {
                tab.phase = Phase::Idle;
            }
            self.cancel_compare_side(id);
            self.cancel_definition(id);
        }
        match action {
            Use::Catalog(id, read) => {
                if let Some(connection) = self
                    .connection_mut(*id)
                    .filter(|item| item.catalog_read == *read)
                {
                    connection.catalog_state = CatalogState::Failed;
                }
            }
            Use::Columns(id, relation) => {
                if let Some(connection) = self.connection_mut(*id) {
                    connection.requested_columns.retain(|item| item != relation);
                }
            }
            Use::Test { test, .. } => {
                if let Some(form) = self
                    .modals
                    .form
                    .as_mut()
                    .filter(|form| form.test == Some(*test))
                {
                    form.test = None;
                    form.close_schema_list();
                }
            }
            _ => {}
        }
    }

    pub(super) fn cancel_active_auth(&mut self) {
        self.auth.job.store(0, Ordering::Release);
        if let Some(attempt) = self.auth.active.take() {
            self.modals.dialogs.retain(|dialog| {
                !dialog
                    .options
                    .iter()
                    .any(|(_, action)| action.auth_token() == Some(attempt.token))
            });
            self.cancel_auth_use(&attempt.action);
            if let Some(connection) = attempt.target.connection {
                let mut queue = std::mem::take(&mut self.auth.queue);
                while let Some(queued) = queue.pop_front() {
                    if queued.target.connection == Some(connection)
                        && queued.target.generation == attempt.target.generation
                    {
                        self.cancel_auth_use(&queued.action);
                    } else {
                        self.auth.queue.push_back(queued);
                    }
                }
            }
        }
        self.auth.prompt = None;
        self.status = Some(text::AUTH_CANCELLED.to_owned());
        self.next_auth();
    }

    /// Cancels only this assistant's request. Other consoles keep their credential requests.
    pub(in crate::app) fn cancel_agent_auth_console(&mut self, id: ConsoleId) {
        let mut queue = std::mem::take(&mut self.auth.queue);
        while let Some(attempt) = queue.pop_front() {
            if attempt.action.console() == Some(id) {
                self.cancel_auth_use(&attempt.action);
            } else {
                self.auth.queue.push_back(attempt);
            }
        }
        if self
            .auth
            .active
            .as_ref()
            .is_some_and(|attempt| attempt.action.console() == Some(id))
        {
            self.auth.job.store(0, Ordering::Release);
            if let Some(attempt) = self.auth.active.take() {
                self.modals.dialogs.retain(|dialog| {
                    !dialog
                        .options
                        .iter()
                        .any(|(_, action)| action.auth_token() == Some(attempt.token))
                });
                self.auth.prompt = None;
                self.cancel_auth_use(&attempt.action);
            }
            self.next_auth();
        }
    }

    pub(in crate::app) fn cancel_auth_console(&mut self, id: ConsoleId) -> bool {
        let mut canceled = false;
        let mut queue = std::mem::take(&mut self.auth.queue);
        while let Some(attempt) = queue.pop_front() {
            if attempt.action.console() == Some(id) {
                self.cancel_auth_use(&attempt.action);
                canceled = true;
            } else {
                self.auth.queue.push_back(attempt);
            }
        }
        if self
            .auth
            .active
            .as_ref()
            .is_some_and(|attempt| attempt.action.console() == Some(id))
        {
            self.cancel_active_auth();
            canceled = true;
        }
        canceled
    }

    pub(in crate::app) fn auth_dialog(&mut self, action: DialogAction) {
        let Some(token) = action.auth_token() else {
            return;
        };
        if self
            .auth
            .active
            .as_ref()
            .is_none_or(|attempt| attempt.token != token)
        {
            return;
        }
        if self
            .auth
            .active
            .as_ref()
            .is_none_or(|attempt| !self.auth_current(attempt))
        {
            self.cancel_active_auth();
            return;
        }
        match action {
            DialogAction::AuthCancel(_) => self.cancel_active_auth(),
            DialogAction::AuthConfirm(_) => self.auth_step(Step::Confirm),
            DialogAction::AuthPlain(_) => self.auth_step(Step::WritePlain),
            DialogAction::AuthMigrate(_) => self.auth_step(Step::Migrate),
            DialogAction::AuthRemovePlain(_) => {
                if let Some(attempt) = self.auth.active.as_mut() {
                    attempt.save_reference = true;
                }
                self.auth_step(Step::RemovePlain);
            }
            DialogAction::AuthRetry(_) => {
                match self.auth.active.as_ref().map(|attempt| attempt.step) {
                    Some(Step::Reference) => self.persist_auth_reference(),
                    Some(step) => self.auth_step(step),
                    None => {}
                }
            }
            DialogAction::AuthSession(_) => {
                if self
                    .auth
                    .active
                    .as_ref()
                    .and_then(|attempt| attempt.target.password())
                    .is_some()
                {
                    self.cache_auth_session();
                } else {
                    self.show_auth_password(false);
                }
            }
            _ => {}
        }
    }
}
