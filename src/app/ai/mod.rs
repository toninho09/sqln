//! Explicit completion coordination. SQL execution and MCP authorization stay separate.
mod explain;

use super::{App, DialogAction};
use crate::ai::{self, Credential, Memory, Preferences, Provider, Snapshot};
use crate::connector::target::Secret;
use crate::creds::SystemVault;
use crate::editor::ai::Target;
use crate::exec::{ConnectionId, ConsoleId};
use crate::input::Action;
use crate::store::Scope;
use crate::store::config;
use crate::ui::ai::RequestPrompt;
use crate::ui::auth::{PasswordAction, PasswordPrompt};
use crate::ui::kit::prompt::TextAction;
use crate::ui::wrap::WrappedText;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use edtui::Index2;
use ratatui::{Frame, layout::Rect, widgets::Paragraph};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

mod form;
mod input;
mod poll;
mod schema;

/// The most Kafka topics whose field paths one snapshot holds. Parallel requests on
/// different topics then share one snapshot and do not cancel the reads of each other.
const FIELD_TOPICS: usize = 4;

struct Cache {
    generation: u64,
    fingerprint: (u64, u64, Option<u64>),
    /// The Kafka topics whose field paths the snapshot holds, the newest last.
    topics: Vec<String>,
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
    kind: RunKind,
    /// The Kafka topic of the statement at the cursor, as typed.
    focus: Option<String>,
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
    completed: Option<Completed>,
    _memory: Memory,
}

/// What a run asks the provider for.
#[derive(Clone)]
enum RunKind {
    /// Ghost text at the anchor of the editor.
    Completion(Target),
    /// An explanation of an error or a plan. The answer shows in a text window.
    Explain(ai::explain::Explanation),
}
/// A response under review in the editor of its console.
struct Completed {
    text: String,
    memory: Option<Memory>,
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
    /// Returns the system vault of the session. Tests can replace it.
    pub(super) fn ai_vault(&self) -> Arc<SystemVault> {
        self.ai.vault.clone()
    }

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
            Action::AiExplainError => {
                self.ai_explain_available() && self.console().messages.has_user_error()
            }
            Action::AiExplainPlan => self.ai_explain_available() && self.console().shows_plan(),
            Action::EditorAiAccept | Action::EditorAiReject => {
                self.console().editor.has_ghosts() && self.review_active()
            }
            Action::EditorAiCancel => {
                self.ai.runs.iter().any(|r| r.console == self.console().id)
                    && (self.editor_has_focus() || self.ai_explanation_running(self.console().id))
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
        // diff review, or a changed requested line ends the run at once. An explanation
        // run needs only its console: its answer shows in a window, not in the editor.
        let mut stale = Vec::new();
        for run in &self.ai.runs {
            let dead = !self.ai.preferences.enabled()
                || self
                    .tabs
                    .consoles
                    .iter()
                    .find(|c| c.id == run.console)
                    .is_none_or(|console| match &run.kind {
                        RunKind::Completion(target) => {
                            console.connection != Some(run.connection)
                                || console.editor.review().is_some()
                                // An edit of the requested line ends the request at once:
                                // the answer would not show.
                                || console.editor.ai_line_row(target).is_none()
                        }
                        RunKind::Explain(_) => {
                            console.connection != Some(run.connection)
                                || !self.console_on_screen()
                                || self.console().id != run.console
                                || console.is_busy()
                        }
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
            let RunKind::Completion(target) = &run.kind else {
                continue;
            };
            let Some(console) = self.tabs.consoles.iter().find(|c| c.id == run.console) else {
                continue;
            };
            if let Some(row) = console.editor.ai_line_row(target) {
                pendings.push((run.console, Index2::new(row, target.start.col), run.started));
            }
        }
        for console in &mut self.tabs.consoles {
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
        if matches!(run.kind, RunKind::Completion(_))
            && let Some(cache) = self.ai.caches.get_mut(&run.connection)
            && cache.loading
            && !cache.background
        {
            cache.cancel.cancel();
            cache.loading = false;
        }
        self.modals.dialogs.retain(|d| {
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
        let Some(connection) = self.console().connection else {
            self.status = Some(crate::ui::text::NO_CONNECTION.to_owned());
            return;
        };
        let effective_schema = self.console().schema.clone().or_else(|| {
            self.active_connection()
                .and_then(|c| c.default_schema.clone())
        });
        let mut target = self.console_mut().editor.ai_target(effective_schema);
        // The request of the user travels with the context. Empty text sends none.
        target.context.instruction = instruction.filter(|text| !text.trim().is_empty());
        let focus = ai::focus_topic(&target.context);
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
            .filter_map(|r| match &r.kind {
                RunKind::Completion(target) => Some((r.id, target)),
                RunKind::Explain(_) => None,
            })
            .filter(|(_, target)| {
                self.tabs
                    .consoles
                    .iter()
                    .find(|c| c.id == console_id)
                    .is_some_and(|c| c.editor.ai_line_row(target) == Some(row))
            })
            .map(|(id, _)| id)
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
        self.push_ai_run(
            RunKind::Completion(target),
            focus,
            provider,
            (scope, base, project),
            connection,
            memory,
        );
    }

    /// Adds one run and starts its consent check. The caller did the checks of its kind.
    fn push_ai_run(
        &mut self,
        kind: RunKind,
        focus: Option<String>,
        provider: Provider,
        location: (String, PathBuf, Option<PathBuf>),
        connection: ConnectionId,
        memory: Memory,
    ) {
        let (scope, base, project) = location;
        let Some(c) = self.connection(connection) else {
            self.status = Some(crate::ui::text::NOT_CONNECTED.to_owned());
            return;
        };
        let identity = c
            .key
            .as_ref()
            .map(|k| format!("{:?}/{}", k.scope, k.id))
            .unwrap_or_else(|| format!("session/{connection}"));
        let endpoint = match provider.endpoint() {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.status = Some(error);
                return;
            }
        };
        let Ok(credential) = serde_json::to_string(&provider.credential) else {
            self.status = Some("The AI credential setting cannot be read.".to_owned());
            return;
        };
        let destination = ai::trust::Destination {
            project,
            connection: identity,
            endpoint,
            credential,
        };
        self.ai.serial += 1;
        let id = self.ai.serial;
        let run = Run {
            id,
            console: self.console().id,
            connection,
            generation: 0,
            kind,
            focus,
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
            completed: None,
            _memory: memory,
        };
        self.console_mut().editor.close_suggestions();
        let explanation = matches!(run.kind, RunKind::Explain(_));
        self.ai.runs.push(run);
        // SQL completion consent does not cover error text or plan predicates.
        if explanation {
            self.ai_consent_ready(id, false);
            return;
        }
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
            let question = if matches!(run.kind, RunKind::Explain(_)) {
                "\n\nAllow this request?"
            } else {
                "\n\nAllow this connection and destination for this project?"
            };
            let (title, body) = match &run.kind {
                RunKind::Completion(_) => (
                    "Send SQL and schema to AI",
                    "SQLN sends SQL near the cursor and schema metadata. Both can contain sensitive information. Table records are not included.".to_owned(),
                ),
                RunKind::Explain(explanation) => (
                    if explanation.label() == "error" {
                        "Send the error to AI"
                    } else {
                        "Send the plan to AI"
                    },
                    match explanation.label() {
                        "error" => "SQLN sends the failed statement, error text, and loaded schema metadata. They can contain sensitive information and literal values. No result rows are sent.".to_owned(),
                        _ => "SQLN sends the statement, query plan, and loaded schema metadata. They can contain sensitive information and literal values. No result rows are sent.".to_owned(),
                    },
                ),
            };
            self.show_dialog(
                title,
                format!(
                    "Connection: {connection}\nEndpoint: {endpoint}\nCredential: {credential}\n\n{body}{question}"
                ),
                vec![
                    ("Cancel", DialogAction::AiCancel(id)),
                    ("Allow", DialogAction::AiAllow(id)),
                ],
            );
            return;
        }
        let (connection, focus) = (run.connection, run.focus.clone());
        match run.kind {
            RunKind::Completion(_) => {
                let generation = self.ensure_ai_schema(connection, focus.as_deref());
                if let Some(run) = self.ai.runs.iter_mut().find(|r| r.id == id) {
                    run.consent = true;
                    run.generation = generation;
                    run.message = "Reading schema";
                }
            }
            RunKind::Explain(_) => {
                // The explanation sends the metadata that is already loaded. It starts
                // no schema read of its own.
                if let Some(run) = self.ai.runs.iter_mut().find(|r| r.id == id) {
                    run.consent = true;
                    run.message = "AI thinking";
                }
            }
        }
        self.continue_ai();
    }
    pub(super) fn ai_allow(&mut self, id: u64) {
        let Some(run) = self.ai.runs.iter().find(|r| r.id == id) else {
            return;
        };
        if matches!(run.kind, RunKind::Explain(_)) {
            self.ai_consent_ready(id, true);
            return;
        }
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
        let Some(run) = self.ai.runs.iter().find(|r| {
            r.consent
                && !r.sending
                && !r.cancel.is_cancelled()
                && (matches!(r.kind, RunKind::Explain(_))
                    || self
                        .ai
                        .caches
                        .get(&r.connection)
                        .and_then(|cache| cache.snapshot.as_ref())
                        .is_some())
        }) else {
            return false;
        };
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
        let Some(run) = self.ai.runs.iter().find(|r| r.id == id) else {
            return;
        };
        let kind = run.kind.clone();
        let provider = run.provider.clone();
        let connection = run.connection;
        let cancel = run.cancel.clone();
        let snapshot = self
            .ai
            .caches
            .get(&connection)
            .and_then(|c| c.snapshot.clone());
        if let Some(run) = self.ai.runs.iter_mut().find(|r| r.id == id) {
            run.message = "AI thinking";
        }
        let sender = self.ai.sender.clone();
        let budget = self.budget.clone();
        let Some(manager) = &self.manager else {
            return;
        };
        match kind {
            RunKind::Completion(target) => {
                let Some(snapshot) = snapshot else {
                    return;
                };
                let context = target.context;
                let context_budget = budget.clone();
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
                        Ok((provider, context, prepared, _hold)) => match ai::client::complete(
                            &provider,
                            &prepared,
                            key.as_ref(),
                            &context.original,
                            &cancel,
                        )
                        .await
                        {
                            Ok(text) => Ok(Completed { text, memory: None }),
                            Err(e) => Err(e),
                        },
                        Err(e) => Err(e),
                    };
                    let _ = sender.send(Work::Done(id, result));
                });
            }
            RunKind::Explain(explanation) => {
                let mut hold = Memory::new(&budget);
                let working = explanation.memory_bytes()
                    + snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.memory_bytes())
                    + provider.context_window_tokens * 4
                    + ai::client::RESPONSE_LIMIT * 4;
                if let Err(error) = hold.charge(working) {
                    let _ = sender.send(Work::Done(id, Err(error)));
                    return;
                }
                manager.spawn_ai(async move {
                    let prepared = tokio::task::spawn_blocking(move || {
                        let prepared = explanation.prepare(&provider, snapshot.as_deref())?;
                        Ok::<_, String>((provider, prepared, hold))
                    })
                    .await;
                    let result = async {
                        let (provider, prepared, _hold) = prepared.map_err(
                            |_| "Cannot prepare the explanation. Reduce the context and try again.",
                        )??;
                        let text =
                            ai::client::ask(&provider, &prepared, key.as_ref(), &cancel).await?;
                        let mut memory = Memory::new(&budget);
                        memory
                            .charge(text.capacity().saturating_add(WrappedText::cache_memory_bound(&text)))
                            .map_err(|_| "The AI answer exceeds the memory budget. Reduce Output tokens or increase the memory budget.")?;
                        Ok(Completed {
                            text,
                            memory: Some(memory),
                        })
                    }
                    .await;
                    let _ = sender.send(Work::Done(id, result));
                });
            }
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
