//! First-use authorization. No pending operation reaches a connector before this gate.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};

use super::{App, DialogAction};
use crate::catalog::RelationRef;
use crate::connection::{CatalogState, ConnectionKey, Definition, Source};
use crate::connector::target::Secret;
use crate::console::Phase;
use crate::creds::{Credentials, Identity, SystemVault, Vault, VaultError};
use crate::editor::RunPlan;
use crate::exec::{ConnectionId, ConsoleId, RunOptions};
use crate::grid::printable;
use crate::store::config::CredentialName;
use crate::store::{Scope, Store};
use crate::table::PageRequest;
use crate::trust::{Required, Trust, TrustError};
use crate::ui::auth::{PasswordAction, PasswordPrompt};
use crate::ui::text;

pub(super) enum Use {
    Catalog(ConnectionId),
    Columns(ConnectionId, RelationRef),
    Run {
        console: ConsoleId,
        connection: ConnectionId,
        plan: RunPlan,
        options: RunOptions,
        revision: u64,
    },
    Page {
        console: ConsoleId,
        connection: ConnectionId,
        request: PageRequest,
    },
    Test(u64),
    Save,
}

impl Use {
    fn console(&self) -> Option<ConsoleId> {
        match self {
            Self::Run { console, .. } | Self::Page { console, .. } => Some(*console),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct Target {
    connection: Option<ConnectionId>,
    generation: u64,
    key: ConnectionKey,
    definition: Definition,
    credential: CredentialName,
    store: Option<Store>,
    project: Option<PathBuf>,
    local: bool,
    new_identity: bool,
    form_snapshot: Option<Definition>,
}

impl Target {
    fn identity(&self) -> Identity {
        Identity::of(&self.key, &self.definition)
            .expect("Only network definitions enter authorization.")
    }

    fn password(&self) -> Option<Secret> {
        match &self.definition.source {
            Source::Network(target) => target.password.clone(),
            _ => None,
        }
    }

    fn set_password(&mut self, password: Option<Secret>) {
        if let Source::Network(target) = &mut self.definition.source {
            target.password = password;
        }
    }
}

#[derive(Clone, Copy)]
enum Step {
    Trust,
    Confirm,
    Read,
    Write,
    Session,
    Reference,
}

struct Attempt {
    token: u64,
    target: Target,
    action: Use,
    step: Step,
    save_reference: bool,
}

enum Reply {
    Trust(Result<Required, TrustError>),
    Password(Result<Option<Secret>, VaultError>),
    Saved(Result<(), VaultError>),
}

pub(super) struct Auth {
    credentials: Arc<Mutex<Credentials<Arc<dyn Vault>>>>,
    job: Arc<AtomicU64>,
    sender: mpsc::Sender<(u64, Reply)>,
    receiver: mpsc::Receiver<(u64, Reply)>,
    queue: VecDeque<Attempt>,
    active: Option<Attempt>,
    pub prompt: Option<PasswordPrompt>,
    ready: HashMap<ConnectionId, u64>,
    pub local: HashMap<ConnectionId, (u64, bool)>,
    next: u64,
}

impl Auth {
    pub fn new(vault: Option<Arc<dyn Vault>>) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            credentials: Arc::new(Mutex::new(Credentials::new(
                vault.unwrap_or_else(|| Arc::new(SystemVault::new())),
            ))),
            job: Arc::new(AtomicU64::new(0)),
            sender,
            receiver,
            queue: VecDeque::new(),
            active: None,
            prompt: None,
            ready: HashMap::new(),
            local: HashMap::new(),
            next: 1,
        }
    }

    pub fn shutdown(&mut self) {
        self.job.store(0, Ordering::Release);
        self.active = None;
        self.prompt = None;
        self.queue.clear();
    }
}

impl App {
    /// Ready connectors and explicit, unsaved definitions are local inputs, not imported configuration.
    pub(super) fn authorize(&mut self, id: ConnectionId, action: Use) {
        if self.config_blocked() {
            return;
        }
        let Some(connection) = self.connection(id) else {
            return;
        };
        let Some(definition) = connection
            .definition
            .clone()
            .filter(|definition| matches!(definition.source, Source::Network(_)))
        else {
            self.dispatch_authorized(action, None);
            return;
        };
        let Some(key) = connection.key.clone() else {
            self.dispatch_authorized(action, Some(definition));
            return;
        };
        if !matches!(action, Use::Save) && self.auth.ready.get(&id) == Some(&connection.generation)
        {
            self.dispatch_authorized(action, Some(definition));
            return;
        }
        let scope = key.scope;
        let target = Target {
            connection: Some(id),
            generation: connection.generation,
            key,
            definition,
            credential: connection.credential,
            store: self.persist.as_ref().map(|persist| persist.store.clone()),
            project: self
                .persist_root(scope)
                .filter(|_| scope == Scope::Project)
                .map(|root| root.base),
            local: self
                .auth
                .local
                .get(&id)
                .is_some_and(|(generation, _)| *generation == connection.generation),
            new_identity: self
                .auth
                .local
                .get(&id)
                .is_some_and(|(generation, new)| *generation == connection.generation && *new),
            form_snapshot: None,
        };
        self.enqueue_auth(target, action);
    }

    pub(super) fn authorize_test(&mut self, mut definition: Definition, test: u64) {
        let Some(form) = &self.form else { return };
        if self.persist.is_none() || !matches!(definition.source, Source::Network(_)) {
            self.dispatch_authorized(Use::Test(test), Some(definition));
            return;
        }
        let snapshot = definition.clone();
        let scope = self.form_scope().unwrap_or(Scope::Global);
        let root = self.persist_root(scope);
        definition.base = root.as_ref().map(|root| root.base.clone());
        let target = Target {
            connection: None,
            generation: 0,
            key: ConnectionKey {
                scope,
                id: form.connection_id.clone(),
            },
            definition,
            credential: form.credential,
            store: self.persist.as_ref().map(|persist| persist.store.clone()),
            project: root
                .filter(|_| scope == Scope::Project)
                .map(|root| root.base),
            local: form.editing.is_none(),
            new_identity: form.editing.is_none(),
            form_snapshot: Some(snapshot),
        };
        self.enqueue_auth(target, Use::Test(test));
    }

    fn enqueue_auth(&mut self, target: Target, action: Use) {
        if let Use::Run { console, plan, .. } = &action {
            let budget = self.budget.clone();
            if let Some(console) = self.console_by_id(*console) {
                console.await_credentials(plan, &budget);
            }
        }
        if let Some(id) = action.console() {
            if let Some(console) = self.console_by_id(id) {
                console.phase = Phase::Waiting;
            }
            if let Some(tab) = self.tables.iter_mut().find(|tab| tab.id == id) {
                tab.phase = Phase::Waiting;
            }
        }
        let token = self.auth.next;
        self.auth.next += 1;
        self.auth.queue.push_back(Attempt {
            token,
            target,
            action,
            step: Step::Trust,
            save_reference: false,
        });
        self.next_auth();
    }

    fn auth_current(&self, attempt: &Attempt) -> bool {
        if self.quit
            || self
                .persist
                .as_ref()
                .is_some_and(|persist| persist.error.is_some())
        {
            return false;
        }
        if let Some(id) = attempt.target.connection {
            return self.connection(id).is_some_and(|connection| {
                connection.generation == attempt.target.generation
                    && connection.key.as_ref() == Some(&attempt.target.key)
            });
        }
        match (&attempt.action, &attempt.target.form_snapshot, &self.form) {
            (Use::Test(test), Some(snapshot), Some(form)) => {
                form.test == Some(*test)
                    && form.connection_id == attempt.target.key.id
                    && self.form_scope() == Some(attempt.target.key.scope)
                    && form.credential == attempt.target.credential
                    && form
                        .definition()
                        .as_ref()
                        .is_ok_and(|definition| definition == snapshot)
            }
            _ => false,
        }
    }

    fn next_auth(&mut self) {
        if self.auth.active.is_some() {
            return;
        }
        while let Some(attempt) = self.auth.queue.pop_front() {
            if !self.auth_current(&attempt) {
                self.cancel_auth_use(&attempt.action);
                continue;
            }
            self.auth.active = Some(attempt);
            self.auth_step(Step::Trust);
            break;
        }
    }

    fn auth_step(&mut self, step: Step) {
        let Some(attempt) = &mut self.auth.active else {
            return;
        };
        attempt.step = step;
        let token = attempt.token;
        let target = attempt.target.clone();
        let credentials = self.auth.credentials.clone();
        let sender = self.auth.sender.clone();
        let job = self.auth.job.clone();
        job.store(token, Ordering::Release);
        self.status = Some(text::AUTH_WAIT.to_owned());
        let Some(manager) = &self.manager else {
            self.cancel_active_auth();
            return;
        };
        manager.spawn_blocking(move || {
            if job.load(Ordering::Acquire) != token {
                return;
            }
            let reply = match step {
                Step::Trust | Step::Confirm => {
                    let result = match target.store.as_ref() {
                        None => Ok(Required {
                            destination: false,
                            project_path: false,
                        }),
                        Some(store) => {
                            let trust = Trust::new(store);
                            if matches!(step, Step::Confirm) {
                                trust
                                    .confirm(&target.identity(), target.project.as_deref())
                                    .map(|()| Required {
                                        destination: false,
                                        project_path: false,
                                    })
                            } else {
                                trust
                                    .required(&target.identity(), target.project.as_deref())
                                    .and_then(|mut required| {
                                        if target.local {
                                            required.destination = false;
                                            if !required.project_path || target.new_identity {
                                                return trust
                                                    .confirm(
                                                        &target.identity(),
                                                        target.project.as_deref(),
                                                    )
                                                    .map(|()| Required {
                                                        destination: false,
                                                        project_path: false,
                                                    });
                                            }
                                        }
                                        Ok(required)
                                    })
                            }
                        }
                    };
                    Reply::Trust(result)
                }
                Step::Read => Reply::Password(
                    credentials
                        .lock()
                        .map_err(|_| VaultError::Failed)
                        .and_then(|mut credentials| credentials.resolve(&target.identity())),
                ),
                Step::Write => {
                    Reply::Saved(credentials.lock().map_err(|_| VaultError::Failed).and_then(
                        |mut credentials| {
                            credentials.save_to_vault(
                                &target.identity(),
                                target.password().ok_or(VaultError::Failed)?,
                            )
                        },
                    ))
                }
                Step::Session => {
                    Reply::Password(credentials.lock().map_err(|_| VaultError::Failed).map(
                        |mut credentials| {
                            let password = target.password();
                            if let Some(password) = &password {
                                credentials.use_for_session(&target.identity(), password.clone());
                            }
                            password
                        },
                    ))
                }
                Step::Reference => return,
            };
            let _ = sender.send((token, reply));
        });
    }

    pub(super) fn poll_auth(&mut self) -> bool {
        let mut changed = false;
        if self
            .auth
            .active
            .as_ref()
            .is_some_and(|attempt| !self.auth_current(attempt))
        {
            self.cancel_active_auth();
            self.status = Some(text::AUTH_CHANGED.to_owned());
            changed = true;
        }
        for _ in 0..64 {
            let Ok((token, reply)) = self.auth.receiver.try_recv() else {
                break;
            };
            if self
                .auth
                .active
                .as_ref()
                .is_none_or(|attempt| attempt.token != token)
            {
                continue;
            }
            changed = true;
            match reply {
                Reply::Trust(Ok(required)) if required.any() => {
                    let target = &self.auth.active.as_ref().unwrap().target;
                    let path = target.project.as_ref().map_or(String::new(), |path| {
                        format!(
                            "\n{}: {}",
                            text::AUTH_PROJECT_LABEL,
                            printable(&path.display().to_string())
                        )
                    });
                    let body = format!(
                        "{}\n{}\n{}{}",
                        printable(&target.definition.name),
                        printable(&target.definition.summary()),
                        text::AUTH_TRUST_BODY,
                        path
                    );
                    let title = if required.project_path && !required.destination {
                        text::TITLE_PROJECT_TRUST
                    } else {
                        text::TITLE_TRUST
                    };
                    self.show_dialog(
                        title,
                        body,
                        vec![
                            (text::OPTION_CANCEL, DialogAction::AuthCancel(token)),
                            (
                                text::OPTION_CONFIRM_DESTINATION,
                                DialogAction::AuthConfirm(token),
                            ),
                        ],
                    );
                }
                Reply::Trust(Ok(_)) => self.after_auth_trust(),
                Reply::Trust(Err(error)) => self.show_dialog(
                    text::TITLE_TRUST,
                    error.to_string(),
                    vec![
                        (text::OPTION_CANCEL, DialogAction::AuthCancel(token)),
                        (text::OPTION_RETRY, DialogAction::AuthRetry(token)),
                    ],
                ),
                Reply::Password(Ok(Some(password))) => self.finish_auth(Some(password)),
                Reply::Password(Ok(None)) => self.show_auth_password(true),
                Reply::Password(Err(error)) => self.auth_vault_error(error),
                Reply::Saved(Ok(())) => self.persist_auth_reference(),
                Reply::Saved(Err(error)) => self.auth_vault_error(error),
            }
        }
        changed
    }

    fn after_auth_trust(&mut self) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        let password = attempt.target.password();
        if matches!(attempt.action, Use::Save) {
            if attempt.target.credential == CredentialName::Vault && password.is_some() {
                self.auth_step(Step::Write);
            } else {
                self.finish_auth(password);
            }
        } else if password.is_some() {
            self.cache_auth_session();
        } else {
            match attempt.target.credential {
                CredentialName::None => self.finish_auth(None),
                CredentialName::Session => self.show_auth_password(true),
                CredentialName::Vault => self.auth_step(Step::Read),
                CredentialName::GlobalPlainText => {
                    let token = attempt.token;
                    self.show_dialog(
                        text::TITLE_VAULT,
                        text::AUTH_PLAINTEXT_PENDING.to_owned(),
                        vec![
                            (text::OPTION_CANCEL, DialogAction::AuthCancel(token)),
                            (text::OPTION_SESSION, DialogAction::AuthSession(token)),
                        ],
                    );
                }
            }
        }
    }

    fn cache_auth_session(&mut self) {
        if let Some(attempt) = &self.auth.active
            && matches!(attempt.action, Use::Test(_))
        {
            let password = attempt.target.password();
            self.finish_auth(password);
        } else {
            self.auth_step(Step::Session);
        }
    }

    fn show_auth_password(&mut self, save_allowed: bool) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        self.auth.prompt = Some(PasswordPrompt::new(
            &attempt.target.definition.name,
            attempt.target.definition.summary(),
            save_allowed && attempt.target.connection.is_some(),
        ));
    }

    fn auth_vault_error(&mut self, error: VaultError) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        let token = attempt.token;
        let note = if matches!(attempt.step, Step::Write) {
            format!("\n{}", text::AUTH_SAVE_FAILED)
        } else {
            String::new()
        };
        self.show_dialog(
            text::TITLE_VAULT,
            format!("{error}{note}"),
            vec![
                (text::OPTION_CANCEL, DialogAction::AuthCancel(token)),
                (text::OPTION_RETRY, DialogAction::AuthRetry(token)),
                (text::OPTION_SESSION, DialogAction::AuthSession(token)),
            ],
        );
    }

    fn persist_auth_reference(&mut self) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        let token = attempt.token;
        if attempt.save_reference && attempt.target.credential != CredentialName::Vault {
            let (key, definition) = (
                attempt.target.key.clone(),
                attempt.target.definition.clone(),
            );
            if self
                .save_connection(&key, &definition, CredentialName::Vault)
                .is_err()
            {
                self.auth.active.as_mut().unwrap().step = Step::Reference;
                self.show_dialog(
                    text::TITLE_VAULT,
                    text::AUTH_REFERENCE_FAILED.to_owned(),
                    vec![
                        (text::OPTION_CANCEL, DialogAction::AuthCancel(token)),
                        (text::OPTION_RETRY, DialogAction::AuthRetry(token)),
                        (text::OPTION_SESSION, DialogAction::AuthSession(token)),
                    ],
                );
                return;
            }
            let id = self.auth.active.as_ref().unwrap().target.connection;
            if let Some(connection) = id.and_then(|id| self.connection_mut(id)) {
                connection.credential = CredentialName::Vault;
            }
            self.auth.active.as_mut().unwrap().target.credential = CredentialName::Vault;
        }
        let password = self
            .auth
            .active
            .as_ref()
            .and_then(|attempt| attempt.target.password());
        self.finish_auth(password);
    }

    fn finish_auth(&mut self, password: Option<Secret>) {
        if self
            .auth
            .active
            .as_ref()
            .is_none_or(|attempt| !self.auth_current(attempt))
        {
            self.cancel_active_auth();
            return;
        }
        let Some(mut attempt) = self.auth.active.take() else {
            return;
        };
        self.auth.job.store(0, Ordering::Release);
        self.auth.prompt = None;
        attempt.target.set_password(password);
        if let Some(id) = attempt.target.connection {
            let definition = attempt.target.definition.clone();
            let changed = self
                .connection(id)
                .and_then(|connection| connection.definition.as_ref())
                != Some(&definition);
            if changed {
                if self
                    .manager
                    .as_ref()
                    .is_some_and(|manager| manager.is_connected(id))
                {
                    self.cancel_auth_use(&attempt.action);
                    self.status = Some(text::SESSION_IN_USE.to_owned());
                    self.next_auth();
                    return;
                }
                if let Some(manager) = &mut self.manager {
                    manager.replace_connection(
                        id,
                        definition.connector(),
                        definition.settings(&self.settings),
                    );
                }
                if let Some(connection) = self.connection_mut(id) {
                    connection.definition = Some(definition);
                }
            }
            if !matches!(attempt.action, Use::Save) {
                self.auth.ready.insert(id, attempt.target.generation);
            }
        }
        self.dispatch_authorized(attempt.action, Some(attempt.target.definition));
        self.next_auth();
    }

    fn dispatch_authorized(&mut self, action: Use, definition: Option<Definition>) {
        let Some(manager) = &self.manager else { return };
        match action {
            Use::Catalog(id) => {
                manager.load_catalog(id);
                if let Some(connection) = self.connection_mut(id) {
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
            } => {
                let Some(index) = self
                    .console_index(console)
                    .filter(|index| self.consoles[*index].connection == Some(connection))
                else {
                    return;
                };
                let execution = manager.run(console, plan.statements.clone(), options);
                self.consoles[index].begin_execution(execution, &plan, &self.budget);
                if self.consoles[index].editor.revision() != revision {
                    self.consoles[index]
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
                    .tables
                    .iter_mut()
                    .find(|tab| tab.id == console && tab.connection == connection)
                else {
                    return;
                };
                let options = RunOptions {
                    manual: false,
                    schema: None,
                    row_cap: Some(request.row_cap),
                };
                let execution = manager.run(console, vec![request.sql], options);
                tab.started(execution, request.first);
            }
            Use::Test(test) => {
                let Some(definition) = definition else { return };
                manager.test_connection(test, definition.connector(), definition.connect_timeout);
            }
            Use::Save => {}
        }
    }

    fn cancel_auth_use(&mut self, action: &Use) {
        if let Some(id) = action.console() {
            if let Some(console) = self.console_by_id(id) {
                console.phase = Phase::Idle;
            }
            if let Some(tab) = self.tables.iter_mut().find(|tab| tab.id == id) {
                tab.phase = Phase::Idle;
            }
        }
        match action {
            Use::Catalog(id) => {
                if let Some(connection) = self.connection_mut(*id) {
                    connection.catalog_state = CatalogState::Failed;
                }
            }
            Use::Columns(id, relation) => {
                if let Some(connection) = self.connection_mut(*id) {
                    connection.requested_columns.retain(|item| item != relation);
                }
            }
            Use::Test(test) => {
                if let Some(form) = self.form.as_mut().filter(|form| form.test == Some(*test)) {
                    form.test = None;
                }
            }
            _ => {}
        }
    }

    fn cancel_active_auth(&mut self) {
        self.auth.job.store(0, Ordering::Release);
        if let Some(attempt) = self.auth.active.take() {
            self.dialogs.retain(|dialog| {
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

    pub(super) fn cancel_auth_console(&mut self, id: ConsoleId) -> bool {
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

    pub(super) fn auth_dialog(&mut self, action: DialogAction) {
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
            DialogAction::AuthRetry(_) => match self.auth.active.as_ref().unwrap().step {
                Step::Reference => self.persist_auth_reference(),
                step => self.auth_step(step),
            },
            DialogAction::AuthSession(_) => {
                if self
                    .auth
                    .active
                    .as_ref()
                    .unwrap()
                    .target
                    .password()
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

    pub(super) fn on_password_key(&mut self, key: KeyEvent) {
        let vim = self.vim();
        let Some(prompt) = &mut self.auth.prompt else {
            return;
        };
        let action = prompt.on_key(key, vim);
        self.password_action(action);
    }

    pub(super) fn on_password_mouse(&mut self, mouse: MouseEvent) {
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

    fn password_action(&mut self, action: PasswordAction) {
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
                let attempt = self.auth.active.as_mut().unwrap();
                attempt.target.set_password(Some(password));
                attempt.save_reference = save;
                if save {
                    self.auth_step(Step::Write);
                } else {
                    self.cache_auth_session();
                }
            }
        }
    }
}

impl DialogAction {
    fn auth_token(&self) -> Option<u64> {
        match self {
            Self::AuthCancel(token)
            | Self::AuthConfirm(token)
            | Self::AuthRetry(token)
            | Self::AuthSession(token) => Some(*token),
            _ => None,
        }
    }
}
