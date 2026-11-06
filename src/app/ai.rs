//! Explicit completion coordination. SQL execution and MCP authorization stay separate.
use super::{App, DialogAction};
use crate::ai::{self, Credential, Memory, Preferences, Provider, Snapshot};
use crate::connector::target::Secret;
use crate::creds::SystemVault;
use crate::editor::ai::Target;
use crate::exec::{ConnectionId, ConsoleId};
use crate::input::Action;
use crate::store::Scope;
use crate::store::config;
use crate::ui::ai::{RequestAction, RequestPrompt};
use crate::ui::auth::{PasswordAction, PasswordPrompt};
use crate::ui::text;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use edtui::Index2;
use ratatui::{Frame, layout::Rect, widgets::Paragraph};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

struct Cache {
    generation: u64,
    fingerprint: (u64, u64, Option<u64>),
    snapshot: Option<Arc<Snapshot>>,
    cancel: CancellationToken,
    loading: bool,
    warmed: bool,
    background: bool,
}
struct Run {
    id: u64,
    console: ConsoleId,
    connection: ConnectionId,
    generation: u64,
    target: Target,
    provider: Provider,
    scope: String,
    base: PathBuf,
    destination: ai::trust::Destination,
    cancel: CancellationToken,
    /// The time of the request, for the animated mark of its anchor.
    started: Instant,
    sending: bool,
    consent: bool,
    message: &'static str,
    session: Option<Secret>,
    _memory: Memory,
}
/// A response under review in the editor of its console.
struct Completed {
    text: String,
}
enum Work {
    Consent(u64, Result<bool, String>),
    Approved(u64, Result<(), String>),
    Key(u64, Result<Option<Secret>, String>),
    Done(u64, Result<Completed, String>),
    Form(u64, Result<(), String>),
    Test(u64, Result<(), String>),
}
pub(super) struct State {
    pub preferences: Preferences,
    runs: Vec<Run>,
    caches: HashMap<ConnectionId, Cache>,
    serial: u64,
    sender: mpsc::Sender<Work>,
    receiver: mpsc::Receiver<Work>,
    schema_sender: mpsc::Sender<ai::schema::Event>,
    schema_receiver: mpsc::Receiver<ai::schema::Event>,
    vault: Arc<SystemVault>,
    form_token: u64,
    form_cancel: CancellationToken,
    prompt: Option<PasswordPrompt>,
    request: Option<RequestPrompt>,
    session_keys: HashMap<String, Secret>,
    pending_save: Option<(config::Preferences, Scope, Secret, String)>,
    cancel_button: Rect,
    approved: Vec<ai::trust::Destination>,
}
impl State {
    pub fn new(preferences: Preferences, vault: Option<Arc<SystemVault>>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let (schema_sender, schema_receiver) = mpsc::channel();
        Self {
            preferences,
            runs: Vec::new(),
            caches: HashMap::new(),
            serial: 0,
            sender,
            receiver,
            schema_sender,
            schema_receiver,
            vault: vault.unwrap_or_else(|| Arc::new(SystemVault::new())),
            form_token: 0,
            form_cancel: CancellationToken::new(),
            prompt: None,
            request: None,
            session_keys: HashMap::new(),
            pending_save: None,
            cancel_button: Rect::default(),
            approved: Vec::new(),
        }
    }
    pub fn busy(&self) -> bool {
        self.runs.iter().any(|r| !r.cancel.is_cancelled())
    }
    pub fn modal(&self) -> bool {
        self.prompt.is_some() || self.request.is_some()
    }
}
impl Drop for State {
    fn drop(&mut self) {
        for run in &self.runs {
            run.cancel.cancel();
        }
        self.form_cancel.cancel();
        for cache in self.caches.values() {
            cache.cancel.cancel();
        }
    }
}
impl App {
    pub(super) fn ai_eligible(&self) -> bool {
        self.ai.preferences.enabled()
            && self.editor_has_focus()
            && self
                .active_connection()
                .is_some_and(|c| c.dialect != crate::sql::Dialect::Redis)
            && !self.console().is_busy()
            && !self.association_pending(self.console().id)
            && self.console().editor.review().is_none()
    }
    pub(super) fn ai_action_available(&self, action: Action) -> bool {
        match action {
            Action::EditorAiComplete | Action::EditorAiCompleteRequest => self.ai_eligible(),
            Action::EditorAiAccept | Action::EditorAiReject => {
                self.console().editor.has_ghosts() && self.review_active()
            }
            Action::EditorAiCancel => {
                self.ai.runs.iter().any(|r| r.console == self.console().id)
                    && self.editor_has_focus()
            }
            _ => false,
        }
    }
    pub(super) fn set_ai_preferences(&mut self, preferences: Preferences) {
        if self.ai.preferences != preferences {
            self.cancel_ai();
            self.ai.preferences = preferences;
            if !self.ai.preferences.enabled() {
                for cache in self.ai.caches.values() {
                    cache.cancel.cancel();
                }
                self.ai.caches.clear();
            }
        }
    }
    pub(super) fn sync_ai(&mut self) {
        let outdated: Vec<_> = self
            .ai
            .caches
            .iter()
            .filter(|(id, c)| self.ai_fingerprint(**id) != Some(c.fingerprint))
            .map(|(id, _)| *id)
            .collect();
        for id in outdated {
            self.invalidate_ai_schema(id);
            self.ai.caches.remove(&id);
        }
        let eligible = self.ai_eligible();
        self.console_mut().editor.set_ai_available(eligible);
        // A ghost run follows its own console: tab changes, busy consoles, schema
        // refreshes, and cursor moves keep it. A closed console, a lost connection, a
        // diff review, or a changed requested line ends the run at once.
        let mut stale = Vec::new();
        for run in &self.ai.runs {
            let dead = !self.ai.preferences.enabled()
                || self
                    .consoles
                    .iter()
                    .find(|c| c.id == run.console)
                    .is_none_or(|console| {
                        console.connection != Some(run.connection)
                            || console.editor.review().is_some()
                            // An edit of the requested line ends the request at once:
                            // the answer would not show.
                            || console.editor.ai_line_row(&run.target).is_none()
                    });
            if dead {
                stale.push(run.id);
            }
        }
        for id in stale {
            self.cancel_ai_id(id);
        }
        if let Some(selection) = self.console_mut().editor.take_ai_request() {
            match selection {
                crate::editor::ai::AiSelection::Plain => self.start_ai_with(None),
                crate::editor::ai::AiSelection::Request => self.open_ai_request(),
            }
        }
        if self.console_mut().editor.take_ai_refused() {
            self.status = Some(crate::ui::text::AI_CURSOR_ONLY.into());
        }
        // The anchors of the running requests show in their console: the user sees
        // where each answer will land while editing other lines.
        let mut pendings: Vec<(ConsoleId, Index2, Instant)> = Vec::new();
        for run in &self.ai.runs {
            let Some(console) = self.consoles.iter().find(|c| c.id == run.console) else {
                continue;
            };
            if let Some(row) = console.editor.ai_line_row(&run.target) {
                pendings.push((
                    run.console,
                    Index2::new(row, run.target.start.col),
                    run.started,
                ));
            }
        }
        for console in &mut self.consoles {
            let list = pendings
                .iter()
                .filter(|(id, _, _)| *id == console.id)
                .map(|(_, at, since)| (*at, *since))
                .collect();
            console.editor.set_ai_pending(list);
        }
    }
    /// Ends one run: its token stops the request, and its dialog closes.
    pub(super) fn cancel_ai_id(&mut self, id: u64) {
        let Some(index) = self.ai.runs.iter().position(|r| r.id == id) else {
            return;
        };
        let run = self.ai.runs.remove(index);
        run.cancel.cancel();
        if let Some(cache) = self.ai.caches.get_mut(&run.connection)
            && cache.loading
            && !cache.background
        {
            cache.cancel.cancel();
            cache.loading = false;
        }
        self.dialogs.retain(|d| {
            !d.options.iter().any(|(_, a)| {
                matches!(
                    a,
                    DialogAction::AiAllow(check)
                        | DialogAction::AiCancel(check)
                        | DialogAction::AiRetry(check)
                        | DialogAction::AiSession(check)
                        if *check == id
                )
            })
        });
        if run.session.is_some() {
            self.ai.prompt = None;
        }
    }
    /// Ends every run: a change of settings, or a closed session, stops them all.
    pub(super) fn cancel_ai(&mut self) {
        let ids: Vec<u64> = self.ai.runs.iter().map(|r| r.id).collect();
        for id in ids {
            self.cancel_ai_id(id);
        }
        self.ai.prompt = None;
    }
    pub(super) fn forget_ai_schema(&mut self, id: ConnectionId) {
        if let Some(cache) = self.ai.caches.remove(&id) {
            cache.cancel.cancel();
        }
    }
    pub(super) fn invalidate_ai_schema(&mut self, id: ConnectionId) {
        let ids: Vec<u64> = self
            .ai
            .runs
            .iter()
            .filter(|r| r.connection == id)
            .map(|r| r.id)
            .collect();
        for run in ids {
            self.cancel_ai_id(run);
        }
        if let Some(cache) = self.ai.caches.get_mut(&id) {
            cache.cancel.cancel();
            cache.snapshot = None;
            cache.loading = false;
        }
    }
    fn ai_fingerprint(&self, id: ConnectionId) -> Option<(u64, u64, Option<u64>)> {
        let c = self.connection(id)?;
        Some((
            c.catalog_read,
            c.generation,
            self.manager.as_ref().and_then(|m| m.pool_serial(id)),
        ))
    }
    pub(super) fn note_ai_catalog_read(&mut self, id: ConnectionId) {
        if let Some(fingerprint) = self.ai_fingerprint(id)
            && let Some(cache) = self.ai.caches.get_mut(&id)
        {
            cache.fingerprint = fingerprint;
        }
    }
    pub(super) fn refresh_ai_schema(&mut self, id: ConnectionId) {
        if self.ai.preferences.enabled() && self.ai.caches.get(&id).is_some_and(|c| c.warmed) {
            self.ensure_ai_schema(id);
            if let Some(cache) = self.ai.caches.get_mut(&id) {
                cache.background = true;
            }
        }
    }
    fn ensure_ai_schema(&mut self, id: ConnectionId) -> u64 {
        let fingerprint = self.ai_fingerprint(id).unwrap_or_default();
        if let Some(cache) = self.ai.caches.get(&id)
            && cache.fingerprint == fingerprint
            && (cache.loading || cache.snapshot.is_some())
        {
            return cache.generation;
        }
        self.ai.serial += 1;
        let generation = self.ai.serial;
        let warmed = self.ai.caches.get(&id).is_some_and(|c| c.warmed);
        if let Some(old) = self.ai.caches.remove(&id) {
            old.cancel.cancel();
        }
        self.ai.caches.insert(
            id,
            Cache {
                generation,
                fingerprint,
                snapshot: None,
                cancel: CancellationToken::new(),
                loading: true,
                warmed,
                background: false,
            },
        );
        self.authorize(id, super::auth::Use::AiSchema(id, generation));
        generation
    }
    pub(super) fn dispatch_ai_schema(&mut self, id: ConnectionId, generation: u64) {
        if let Some(fingerprint) = self.ai_fingerprint(id)
            && let Some(cache) = self.ai.caches.get_mut(&id)
        {
            cache.fingerprint = fingerprint;
        }
        let Some(cache) = self
            .ai
            .caches
            .get(&id)
            .filter(|c| c.generation == generation && !c.cancel.is_cancelled())
        else {
            return;
        };
        let schemas = self
            .connection(id)
            .and_then(|c| c.definition.as_ref())
            .map(|d| d.schemas.clone())
            .unwrap_or_default();
        if let Some(manager) = &self.manager {
            manager.load_ai_schema(
                id,
                generation,
                schemas,
                cache.cancel.clone(),
                self.ai.schema_sender.clone(),
            );
        }
    }
    pub(super) fn cancel_ai_schema_auth(&mut self, id: ConnectionId, generation: u64) {
        if let Some(c) = self
            .ai
            .caches
            .get_mut(&id)
            .filter(|c| c.generation == generation)
        {
            c.loading = false;
            c.cancel.cancel();
        }
        let dead: Vec<u64> = self
            .ai
            .runs
            .iter()
            .filter(|r| r.connection == id && r.generation == generation)
            .map(|r| r.id)
            .collect();
        if !dead.is_empty() {
            for run in dead {
                self.cancel_ai_id(run);
            }
            self.status=Some("Schema authorization cancelled. Request completion again when access is available.".into());
        }
    }
    fn ai_location(&self) -> Result<(String, PathBuf, Option<PathBuf>), String> {
        let scope = self.ai_provider_scope();
        match &self.persist {
            Some(p) => {
                let project = p
                    .store
                    .project
                    .as_ref()
                    .map(|r| r.base.canonicalize())
                    .transpose()
                    .map_err(|_| "Cannot resolve the project folder. Check its path.")?;
                let root = p
                    .store
                    .root(scope)
                    .ok_or("The settings scope is not available.")?;
                Ok((
                    if scope == Scope::Project {
                        format!(
                            "project:{}",
                            project.as_ref().ok_or("No project folder.")?.display()
                        )
                    } else {
                        "global".into()
                    },
                    root.base.clone(),
                    project,
                ))
            }
            None => Ok((
                "session".into(),
                std::env::current_dir().map_err(|_| "Cannot resolve the current folder.")?,
                None,
            )),
        }
    }
    fn ai_provider_scope(&self) -> Scope {
        self.persist
            .as_ref()
            .and_then(|p| p.store.project.as_ref())
            .and_then(|r| crate::store::config::preferences(r, Scope::Project).ok())
            .and_then(|p| p.ai_completion)
            .filter(|p| p.provider.is_some())
            .map_or(Scope::Global, |_| Scope::Project)
    }
    /// Opens the request input of a completion. The send runs the plain checks.
    pub(super) fn open_ai_request(&mut self) {
        if !self.ai_eligible() || self.config_blocked() {
            self.status = Some(
                "AI completion is unavailable. Enable it for a SQL console in Settings > Autocomplete."
                    .into(),
            );
            return;
        }
        if self.ai.prompt.is_some() || self.ai.request.is_some() {
            return;
        }
        self.console_mut().editor.close_suggestions();
        self.ai.request = Some(RequestPrompt::new());
    }
    pub(super) fn start_ai_with(&mut self, instruction: Option<String>) {
        if !self.ai_eligible() || self.config_blocked() {
            self.status=Some("AI completion is unavailable. Enable it for a SQL console in Settings > Autocomplete.".into());
            return;
        }
        let Some(provider) = self.ai.preferences.provider.clone() else {
            self.status = Some("Configure a provider in Settings > Autocomplete.".into());
            return;
        };
        if let Err(error) = provider.validate() {
            self.status = Some(error);
            return;
        }
        let (scope, base, project) = match self.ai_location() {
            Ok(v) => v,
            Err(e) => {
                self.status = Some(e);
                return;
            }
        };
        let connection = self.console().connection.expect("eligible connection");
        let effective_schema = self.console().schema.clone().or_else(|| {
            self.active_connection()
                .and_then(|c| c.default_schema.clone())
        });
        let mut target = self.console_mut().editor.ai_target(effective_schema);
        // The request of the user travels with the context. Empty text sends none.
        target.context.instruction = instruction.filter(|text| !text.trim().is_empty());
        let console_id = self.console().id;
        // The AI completion works at the cursor only: a selection never starts it.
        if self.console().editor.selection_range().is_some() {
            self.status = Some(crate::ui::text::AI_CURSOR_ONLY.into());
            return;
        }
        // A request on a line replaces the answer of that line, then checks the
        // limit of answers that wait in one console.
        let row = target.start.row;
        self.console_mut().editor.close_ghosts_at_row(row);
        let same_line: Vec<u64> = self
            .ai
            .runs
            .iter()
            .filter(|r| r.console == console_id)
            .filter(|r| {
                self.consoles
                    .iter()
                    .find(|c| c.id == console_id)
                    .is_some_and(|c| c.editor.ai_line_row(&r.target) == Some(row))
            })
            .map(|r| r.id)
            .collect();
        for id in same_line {
            self.cancel_ai_id(id);
        }
        let limit = self.ai.preferences.max_simultaneous();
        let waiting = self.console().editor.ghosts().len()
            + self
                .ai
                .runs
                .iter()
                .filter(|r| r.console == console_id)
                .count();
        if waiting >= limit {
            self.status = Some(format!(
                "AI completion limit reached ({limit}). Accept or reject a pending answer first."
            ));
            return;
        }
        let mut memory = Memory::new(&self.budget);
        if let Err(e) = memory.charge(
            (target.context.prefix.len()
                + target.context.original.len()
                + target.context.suffix.len()
                + target
                    .context
                    .instruction
                    .as_ref()
                    .map_or(0, |text| text.len()))
                * 2
                + 1024,
        ) {
            self.status = Some(e);
            return;
        }
        let c = self.connection(connection).expect("eligible connection");
        let identity = c
            .key
            .as_ref()
            .map(|k| format!("{:?}/{}", k.scope, k.id))
            .unwrap_or_else(|| format!("session/{connection}"));
        let destination = ai::trust::Destination {
            project,
            connection: identity,
            endpoint: provider.endpoint().expect("validated"),
            credential: serde_json::to_string(&provider.credential).expect("credential"),
        };
        self.ai.serial += 1;
        let id = self.ai.serial;
        let run = Run {
            id,
            console: self.console().id,
            connection,
            generation: 0,
            target,
            provider,
            scope,
            base,
            destination: destination.clone(),
            cancel: CancellationToken::new(),
            started: Instant::now(),
            sending: false,
            consent: false,
            message: "Checking AI consent",
            session: None,
            _memory: memory,
        };
        self.console_mut().editor.close_suggestions();
        self.ai.runs.push(run);
        if self.ai.approved.contains(&destination) {
            self.ai_consent_ready(id, true);
            return;
        }
        let root = self.persist.as_ref().map(|p| p.store.global.dir.clone());
        let sender = self.ai.sender.clone();
        if let Some(manager) = &self.manager {
            manager.spawn_blocking(move || {
                let result =
                    root.map_or(Ok(false), |root| ai::trust::approved(&root, &destination));
                let _ = sender.send(Work::Consent(id, result));
            });
        }
    }
    fn ai_consent_ready(&mut self, id: u64, approved: bool) {
        let Some(run) = self.ai.runs.iter().find(|r| r.id == id) else {
            return;
        };
        if !approved {
            let endpoint = crate::grid::printable(&run.destination.endpoint);
            let connection = self
                .connection(run.connection)
                .map(|c| crate::grid::printable(c.plain_name()))
                .unwrap_or_default();
            let credential = match &run.provider.credential {
                Credential::None => "None".into(),
                Credential::Vault { .. } => "System vault".into(),
                Credential::File { path } => format!(
                    "Key file: {}",
                    crate::grid::printable(
                        &ai::credentials::file_path(&run.base, path)
                            .display()
                            .to_string()
                    )
                ),
            };
            self.show_dialog("Send SQL and schema to AI",format!("Connection: {connection}\nEndpoint: {endpoint}\nCredential: {credential}\n\nSQLN sends SQL near the cursor and schema metadata. Both can contain sensitive information. Table records are not included.\n\nAllow this connection and destination for this project?"),vec![("Cancel",DialogAction::AiCancel(id)),("Allow",DialogAction::AiAllow(id))]);
            return;
        }
        let connection = run.connection;
        let generation = self.ensure_ai_schema(connection);
        if let Some(run) = self.ai.runs.iter_mut().find(|r| r.id == id) {
            run.consent = true;
            run.generation = generation;
            run.message = "Reading schema";
        }
        self.continue_ai();
    }
    pub(super) fn ai_allow(&mut self, id: u64) {
        let Some(run) = self.ai.runs.iter().find(|r| r.id == id) else {
            return;
        };
        let destination = run.destination.clone();
        let root = self.persist.as_ref().map(|p| p.store.global.dir.clone());
        let sender = self.ai.sender.clone();
        if let Some(manager) = &self.manager {
            manager.spawn_blocking(move || {
                let result = root.map_or(Ok(()), |root| ai::trust::confirm(&root, destination));
                let _ = sender.send(Work::Approved(id, result));
            });
        }
    }
    /// Continues every ready run: each one with consent, without a task, and with a
    /// cached schema reads its key and sends. A selection waits for its review.
    fn continue_ai(&mut self) {
        while self.continue_one_ai() {}
    }
    fn continue_one_ai(&mut self) -> bool {
        let Some(run) = self
            .ai
            .runs
            .iter()
            .find(|r| r.consent && !r.sending && !r.cancel.is_cancelled())
        else {
            return false;
        };
        if self
            .ai
            .caches
            .get(&run.connection)
            .and_then(|c| c.snapshot.as_ref())
            .is_none()
        {
            return false;
        }
        let (provider, scope, base, id) = (
            run.provider.clone(),
            run.scope.clone(),
            run.base.clone(),
            run.id,
        );
        let key = run
            .session
            .clone()
            .or_else(|| match &run.provider.credential {
                Credential::Vault { id, endpoint } => self
                    .ai
                    .session_keys
                    .get(&ai::credentials::account(&run.scope, id, endpoint))
                    .cloned(),
                _ => None,
            });
        let vault = self.ai.vault.clone();
        let sender = self.ai.sender.clone();
        let cancel = run.cancel.clone();
        if let Some(run) = self.ai.runs.iter_mut().find(|r| r.id == id) {
            run.sending = true;
            run.message = "Reading API key";
        }
        if let Some(manager) = &self.manager {
            manager.spawn_blocking(move || {
                if cancel.is_cancelled() {
                    return;
                }
                let result = if key.is_some() {
                    Ok(key)
                } else {
                    ai::credentials::read(&provider, &scope, &base, &vault)
                };
                let _ = sender.send(Work::Key(id, result));
            });
        }
        true
    }
    fn send_ai(&mut self, id: u64, key: Option<Secret>) {
        let Some(run) = self.ai.runs.iter_mut().find(|r| r.id == id) else {
            return;
        };
        let Some(snapshot) = self
            .ai
            .caches
            .get(&run.connection)
            .and_then(|c| c.snapshot.clone())
        else {
            return;
        };
        run.message = "AI thinking";
        let (provider, context, cancel) = (
            run.provider.clone(),
            run.target.context.clone(),
            run.cancel.clone(),
        );
        let sender = self.ai.sender.clone();
        let budget = self.budget.clone();
        let context_budget = budget.clone();
        if let Some(manager) = &self.manager {
            manager.spawn_ai(async move {
                let prepared = tokio::task::spawn_blocking(move || {
                    let mut hold = Memory::new(&context_budget);
                    hold.charge(
                        context.prefix.len()
                            + context.original.len()
                            + context.suffix.len()
                            + provider.context_window_tokens * 2,
                    )?;
                    let prepared = ai::prepare(&provider, &context, &snapshot)?;
                    Ok::<_, String>((provider, context, prepared, hold))
                })
                .await
                .map_err(|_| "Cannot prepare AI context. Try a smaller selection.".to_owned())
                .and_then(|r| r);
                let result = match prepared {
                    Ok((provider, context, prepared, _hold)) => {
                        match ai::client::complete(
                            &provider,
                            &prepared,
                            key.as_ref(),
                            &context.original,
                            &cancel,
                        )
                        .await
                        {
                            Ok(text) => Ok(Completed { text }),
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                };
                let _ = sender.send(Work::Done(id, result));
            });
        }
    }
    /// Shows a response in the editor: ghost text at its anchor for an insertion, an
    /// inline diff for a replacement.
    fn show_ai_proposal(&mut self, id: u64, completed: Completed) {
        let Some(run) = self.ai.runs.iter().find(|r| r.id == id) else {
            return;
        };
        let (console, target) = (run.console, run.target.clone());
        let Some(index) = self.console_index(console) else {
            self.cancel_ai_id(id);
            return;
        };
        // The answer shows at the requested anchor. An edit of that line, and
        // only that, discards it; an edit elsewhere moves the anchor.
        let Some(row) = self.consoles[index].editor.ai_line_row(&target) else {
            self.cancel_ai_id(id);
            return;
        };
        let mut at = target.start;
        at.row = row;
        let keys = self.ghost_keys();
        self.consoles[index].editor.set_ghost_keys(keys);
        self.consoles[index].editor.start_ghost(at, &completed.text);
    }
    pub(super) fn ai_retry(&mut self, id: u64) {
        if let Some(run) = self.ai.runs.iter_mut().find(|r| r.id == id) {
            run.sending = false;
        }
        self.continue_ai();
    }
    pub(super) fn ai_session(&mut self, id: u64) {
        if self.ai.runs.iter().any(|r| r.id == id) {
            self.ai.prompt = Some(PasswordPrompt::new(
                "AI session key",
                "Use only for this completion. Nothing is saved.".into(),
                false,
            ));
        }
    }
    /// Decides the ghost at the line of the cursor: the cursor line names the answer.
    /// A line without a ghost explains the rule.
    pub(super) fn decide_ghost(&mut self, accept: bool) {
        let row = self.console().editor.cursor().row;
        let Some(index) = self.console_mut().editor.ghost_at(row) else {
            self.status = Some("No AI answer at the line of the cursor.".into());
            return;
        };
        if let Err(error) = self.console_mut().editor.ghost_decide(index, accept) {
            self.status = Some(error);
        }
    }

    /// Escapes at the line of the cursor: the run of that line ends, and a ghost
    /// there closes. A line without either keeps everything.
    pub(super) fn escape_ghost(&mut self) {
        let row = self.console().editor.cursor().row;
        let console = self.console().id;
        let ids: Vec<u64> = self
            .ai
            .runs
            .iter()
            .filter(|r| {
                r.console == console
                    && self
                        .consoles
                        .iter()
                        .find(|c| c.id == console)
                        .is_some_and(|c| c.editor.ai_line_row(&r.target) == Some(row))
            })
            .map(|r| r.id)
            .collect();
        if !ids.is_empty() {
            for id in ids {
                self.cancel_ai_id(id);
            }
            return;
        }
        if let Some(index) = self.console_mut().editor.ghost_at(row) {
            let _ = self.console_mut().editor.ghost_decide(index, false);
        }
    }

    pub(super) fn poll_ai(&mut self) -> bool {
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
                Work::Done(id, result) if self.ai.runs.iter().any(|r| r.id == id) => {
                    match result {
                        Ok(completed) => {
                            self.show_ai_proposal(id, completed);
                            // The answer ended the run; its answer now waits in the
                            // editor.
                            self.cancel_ai_id(id);
                        }
                        Err(e) => {
                            self.cancel_ai_id(id);
                            self.status = Some(e);
                        }
                    }
                }
                Work::Form(token, result) if token == self.ai.form_token => {
                    if let Some(form) = &mut self.settings_form {
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
                    if let Some(form) = &mut self.settings_form {
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
                            if let Some(form) = &mut self.settings_form {
                                form.note = Some(e);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.continue_ai();
        changed
    }
    /// Closes the settings and stops a vault write or a test of the provider.
    pub(super) fn close_settings(&mut self) {
        self.settings_form = None;
        self.ai.form_token += 1;
        self.ai.form_cancel.cancel();
        self.ai.pending_save = None;
    }

    /// The vault write failed. The save starts again from the current fields.
    pub(super) fn ai_form_retry(&mut self) {
        self.ai.pending_save = None;
        self.save_settings();
    }
    pub(super) fn ai_form_session(&mut self) {
        let Some((preferences, scope, key, account)) = self.ai.pending_save.take() else {
            return;
        };
        self.ai.session_keys.insert(account, key);
        if let Some(ai) = preferences.ai_completion.clone()
            && let Some(form) = &mut self.settings_form
        {
            form.set_ai_preferences(ai);
        }
        self.write_settings(preferences, scope);
        self.status = Some("The API key stays in this session only.".into());
    }
    /// Saves the settings. A typed vault key goes to the vault first; the save
    /// continues when the write ends.
    pub(super) fn save_settings(&mut self) {
        let Some(form) = &mut self.settings_form else {
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
    /// Writes the scope of the dialog. The caller built and validated the values.
    pub(super) fn write_settings(&mut self, preferences: config::Preferences, scope: Scope) {
        let Some(persist) = &mut self.persist else {
            return;
        };
        // Use the current files for the other scope, not only the dialog snapshot.
        let validation = match scope {
            Scope::Global => persist
                .store
                .project
                .as_ref()
                .map(|root| config::preferences(root, Scope::Project))
                .transpose()
                .and_then(|project| config::validate_keymaps(&preferences, project.as_ref())),
            Scope::Project => config::preferences(&persist.store.global, Scope::Global)
                .and_then(|global| config::validate_keymaps(&global, Some(&preferences))),
        };
        if let Err(message) = validation {
            if let Some(form) = &mut self.settings_form {
                form.note = Some(message);
            }
            return;
        }
        let Some(root) = persist.store.root(scope) else {
            return;
        };
        match config::change_preferences(root, scope, preferences) {
            Ok(hash) => {
                if let Some((_, known)) = persist.hashes.iter_mut().find(|(item, _)| *item == scope)
                {
                    *known = Some(hash);
                }
                self.settings_form = None;
                self.reload_config();
                if self
                    .persist
                    .as_ref()
                    .is_some_and(|persist| persist.error.is_none())
                {
                    self.status = Some(text::SETTINGS_SAVED.to_owned());
                }
            }
            Err(message) => {
                if let Some(form) = &mut self.settings_form {
                    form.note = Some(message);
                }
            }
        }
    }
    /// The vault account of one scope and credential endpoint.
    fn ai_account(&self, scope: Scope, id: &str, endpoint: &str) -> String {
        let root = self.persist_root(scope);
        let base = root.as_ref().map(|r| r.base.clone()).unwrap_or_default();
        let scope_name = self.ai_scope_name(scope, &base);
        ai::credentials::account(&scope_name, id, endpoint)
    }

    /// The vault scope name of one scope.
    fn ai_scope_name(&self, scope: Scope, base: &std::path::Path) -> String {
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
    pub(super) fn test_ai_connection(&mut self) {
        let Some(form) = &mut self.settings_form else {
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
    fn ai_obscured(&self) -> bool {
        !self.dialogs.is_empty()
            || self.auth.prompt.is_some()
            || self.conflicts.is_some()
            || self.palette.is_some()
            || self.history.is_some()
            || self.rename.is_some()
            || self.browser.is_some()
            || self.settings_form.is_some()
            || self.form.is_some()
            || self.export_form.is_some()
            || self.edit_form.is_some()
            || self.import_form.is_some()
            || self.compare_form.is_some()
            || self.help.is_some()
            || self.overlay.is_some()
            || self.picker.is_some()
            || self.command_page.is_some()
            || self.pending_key.is_some()
    }
    pub(super) fn on_ai_key(&mut self, key: KeyEvent) -> bool {
        if let Some(prompt) = &mut self.ai.request {
            match prompt.on_key(key) {
                RequestAction::Cancel => self.ai.request = None,
                RequestAction::Accept(text) => {
                    self.ai.request = None;
                    let instruction = if text.trim().is_empty() {
                        None
                    } else {
                        Some(text)
                    };
                    self.start_ai_with(instruction);
                }
                _ => {}
            }
            return true;
        }
        if let Some(prompt) = &mut self.ai.prompt {
            match prompt.on_key(key, false) {
                PasswordAction::Cancel => self.cancel_ai(),
                PasswordAction::Submit { password, .. } => {
                    self.ai.prompt = None;
                    if let Some(run) = self.ai.runs.iter_mut().find(|r| r.session.is_none()) {
                        run.session = Some(password);
                        run.sending = false;
                    }
                    self.continue_ai();
                }
                _ => {}
            }
            return true;
        }
        if self.ai_obscured() {
            return false;
        }
        if let crate::input::Lookup::Action(action) = self.keymap.lookup(
            crate::input::Region::Editor,
            crate::input::Chord::from_event(key),
        ) && matches!(
            action,
            Action::EditorAiAccept | Action::EditorAiReject | Action::EditorAiCancel
        ) && self.ai_action_available(action)
        {
            match action {
                Action::EditorAiAccept => self.decide_ghost(true),
                Action::EditorAiReject => self.decide_ghost(false),
                _ => self.cancel_ai(),
            }
            return true;
        }
        if key.code == KeyCode::Esc
            && self.ai.runs.iter().any(|r| r.console == self.console().id)
            && self.editor_has_focus()
        {
            self.escape_ghost();
            return true;
        }
        false
    }
    pub(super) fn on_ai_mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.ai.request.is_some() {
            // The request input is a dialog: clicks under it start nothing.
            return true;
        }
        if self.ai.prompt.is_some() {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && let Some((field, _)) = self
                    .layout
                    .password
                    .iter()
                    .find(|(_, r)| r.contains((mouse.column, mouse.row).into()))
            {
                let action = self.ai.prompt.as_mut().unwrap().click(*field);
                match action {
                    PasswordAction::Cancel => self.cancel_ai(),
                    PasswordAction::Submit { password, .. } => {
                        self.ai.prompt = None;
                        if let Some(run) = self.ai.runs.iter_mut().find(|r| r.session.is_none()) {
                            run.session = Some(password);
                            run.sending = false;
                        }
                        self.continue_ai();
                    }
                    _ => {}
                }
            }
            return true;
        }
        if self.ai_obscured() {
            return false;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self
                .ai
                .cancel_button
                .contains((mouse.column, mouse.row).into())
            && self.ai.runs.iter().any(|r| r.console == self.console().id)
        {
            // The bar belongs to the console on screen, so its button ends the runs
            // of that console.
            let console = self.console().id;
            let ids: Vec<u64> = self
                .ai
                .runs
                .iter()
                .filter(|r| r.console == console)
                .map(|r| r.id)
                .collect();
            for id in ids {
                self.cancel_ai_id(id);
            }
            return true;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self.editor_has_focus()
            && self
                .console_mut()
                .editor
                .click_suggestion(mouse.column, mouse.row)
        {
            return true;
        }
        false
    }
    pub(super) fn ai_paste(&mut self, text: &str) -> bool {
        if let Some(prompt) = &mut self.ai.request {
            prompt.paste(text);
            return true;
        }
        if let Some(prompt) = &mut self.ai.prompt {
            prompt.paste(text);
            return true;
        }
        false
    }
    pub(super) fn draw_ai(&mut self, frame: &mut Frame) {
        self.ai.cancel_button = Rect::default();
        let console = self.console().id;
        let runs: Vec<&Run> = self
            .ai
            .runs
            .iter()
            .filter(|r| r.console == console)
            .collect();
        let Some(last) = runs.last().copied() else {
            return;
        };
        let area = self.layout.console.editor;
        if area.width > 0 && area.height > 0 {
            self.ai.cancel_button =
                Rect::new(area.right().saturating_sub(10), area.bottom() - 1, 10, 1);
            let spin = ["|", "/", "-", "\\"][self.run_frame % 4];
            // One run names its step; several show their count.
            let state = if runs.len() == 1 {
                last.message.to_owned()
            } else {
                format!("thinking ×{}", runs.len())
            };
            frame.render_widget(
                Paragraph::new(format!(" AI {spin} {state} · Esc: Cancel ")).style(self.theme.hint),
                Rect {
                    x: area.x,
                    y: area.bottom() - 1,
                    width: area.width,
                    height: 1,
                },
            );
            frame.render_widget(
                Paragraph::new(self.theme.button("Cancel")).style(self.theme.dialog_option),
                self.ai.cancel_button,
            );
        }
    }
    /// Draws the dialogs of the AI runs: the vault key prompt and the request input.
    pub(super) fn draw_ai_prompts(&mut self, frame: &mut Frame) {
        if let Some(prompt) = &self.ai.request {
            crate::ui::ai::draw_request(frame, prompt, &self.theme);
        }
        if let Some(prompt) = &self.ai.prompt {
            self.layout.password = crate::ui::auth::draw_password(frame, prompt, &self.theme);
        }
    }
}
