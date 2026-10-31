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
use crate::connector::ConnectError;
use crate::creds::{Credentials, Identity, Lookup, PlainFile, SystemVault, Vault, VaultError};
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
    /// The connection and the number of the read.
    Catalog(ConnectionId, u64),
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
    /// Asks for a new password after a refusal. It runs nothing.
    NewPassword,
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
    /// The database refused the password. A new one needs a check before any other use.
    refused: bool,
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
    WritePlain,
    Session,
    Reference,
    Verify,
    Migrate,
    RemovePlain,
}

struct Attempt {
    token: u64,
    target: Target,
    action: Use,
    step: Step,
    save_reference: bool,
    /// This computer has no vault, so a save goes to the plain text file.
    no_vault: bool,
    /// The identity of the connection test that checks a new password.
    verify: Option<u64>,
}

enum Reply {
    Trust(Result<Required, TrustError>),
    Lookup(Result<Lookup, VaultError>),
    Password(Result<Option<Secret>, VaultError>),
    Saved(Result<(), VaultError>),
    Migrated(Result<(), VaultError>),
    PlainRemoved(Result<bool, VaultError>),
    /// A separate deletion. It has no attempt, so its token is `DELETION`.
    Deleted(Result<bool, VaultError>),
}

const DELETION: u64 = 0;

/// The refusals of one definition. A changed definition ends them.
#[derive(Clone, Copy)]
struct Refusal {
    generation: u64,
    count: u32,
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
    refused: HashMap<ConnectionId, Refusal>,
    next: u64,
}

impl Auth {
    pub fn new(vault: Option<Arc<dyn Vault>>, plain: Option<PlainFile>) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            credentials: Arc::new(Mutex::new(
                Credentials::new(vault.unwrap_or_else(|| Arc::new(SystemVault::new())))
                    .with_plain(plain),
            )),
            job: Arc::new(AtomicU64::new(0)),
            sender,
            receiver,
            queue: VecDeque::new(),
            active: None,
            prompt: None,
            ready: HashMap::new(),
            local: HashMap::new(),
            refused: HashMap::new(),
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
        let refused = self.is_refused(id) && !matches!(action, Use::Save);
        // Only an action of the user can reach the database after a refusal.
        if refused && matches!(action, Use::Catalog(..) | Use::Columns(..)) {
            self.cancel_auth_use(&action);
            return;
        }
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
            refused,
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
            refused: false,
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
                tab.run.wait();
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
            no_vault: false,
            verify: None,
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
                    && self.form_definition().is_some_and(|definition| definition.as_ref() == Ok(snapshot))
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

    /// Returns the message of a credential step in the background. It has no time limit,
    /// because the end of the step ends the message.
    pub(super) fn auth_wait(&self) -> Option<&'static str> {
        let asks = self.auth.prompt.is_some() || !self.dialogs.is_empty();
        (self.auth.active.is_some() && !asks).then_some(text::AUTH_WAIT)
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
                Step::Read => {
                    let reference = target.credential == CredentialName::Vault;
                    Reply::Lookup(
                        credentials
                            .lock()
                            .map_err(|_| VaultError::Failed)
                            .and_then(|mut credentials| credentials.lookup(&target.identity(), reference)),
                    )
                }
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
                Step::WritePlain => {
                    Reply::Saved(credentials.lock().map_err(|_| VaultError::Failed).and_then(
                        |mut credentials| {
                            credentials.save_plain(
                                &target.identity(),
                                target.password().ok_or(VaultError::Failed)?,
                            )
                        },
                    ))
                }
                Step::Migrate => {
                    Reply::Migrated(credentials.lock().map_err(|_| VaultError::Failed).and_then(
                        |mut credentials| {
                            credentials.save_verified(
                                &target.identity(),
                                target.password().ok_or(VaultError::Failed)?,
                            )
                        },
                    ))
                }
                Step::RemovePlain => Reply::PlainRemoved(
                    credentials
                        .lock()
                        .map_err(|_| VaultError::Failed)
                        .and_then(|mut credentials| credentials.remove_plain(&target.identity())),
                ),
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
                Step::Reference | Step::Verify => return,
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
            if let Reply::Deleted(result) = reply {
                self.status = Some(match result {
                    Ok(true) => text::PASSWORD_DELETED.to_owned(),
                    Ok(false) => text::PASSWORD_NOT_FOUND.to_owned(),
                    Err(error) => error.to_string(),
                });
                changed = true;
                continue;
            }
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
                Reply::Lookup(Ok(Lookup::Found(password))) => self.finish_auth(Some(password)),
                Reply::Lookup(Ok(Lookup::Missing)) => self.show_auth_password(true),
                Reply::Lookup(Ok(Lookup::NoVault)) => {
                    if let Some(attempt) = &mut self.auth.active {
                        attempt.no_vault = true;
                    }
                    self.show_auth_password(true);
                }
                Reply::Lookup(Ok(Lookup::Migrate(password))) => self.ask_migration(password),
                Reply::Password(Ok(Some(password))) => self.finish_auth(Some(password)),
                Reply::Password(Ok(None)) => self.show_auth_password(true),
                Reply::Lookup(Err(error)) | Reply::Password(Err(error)) => self.auth_vault_error(error),
                Reply::Saved(Ok(())) => self.persist_auth_reference(),
                Reply::Saved(Err(error)) => self.auth_vault_error(error),
                Reply::Migrated(Ok(())) => self.show_dialog(
                    text::TITLE_MIGRATION,
                    text::MIGRATION_VERIFIED.to_owned(),
                    vec![
                        (text::OPTION_KEEP_PLAIN, DialogAction::AuthSession(token)),
                        (text::OPTION_REMOVE_PLAIN, DialogAction::AuthRemovePlain(token)),
                    ],
                ),
                Reply::Migrated(Err(error)) => self.show_dialog(
                    text::TITLE_MIGRATION,
                    format!("{error}\n{}", text::MIGRATION_INCOMPLETE),
                    vec![
                        (text::OPTION_SESSION, DialogAction::AuthSession(token)),
                        (text::OPTION_RETRY, DialogAction::AuthRetry(token)),
                    ],
                ),
                Reply::PlainRemoved(Ok(_)) => self.persist_auth_reference(),
                Reply::PlainRemoved(Err(error)) => self.show_dialog(
                    text::TITLE_MIGRATION,
                    format!("{error}\n{}", text::MIGRATION_INCOMPLETE),
                    vec![
                        (text::OPTION_SESSION, DialogAction::AuthSession(token)),
                        (text::OPTION_RETRY, DialogAction::AuthRetry(token)),
                    ],
                ),
                Reply::Deleted(_) => {}
            }
        }
        changed
    }

    fn after_auth_trust(&mut self) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        let password = attempt.target.password();
        if attempt.target.refused {
            // The stored password is the refused one, so no store can answer.
            self.show_auth_password(true);
        } else if matches!(attempt.action, Use::Save) {
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
                CredentialName::Session | CredentialName::Vault | CredentialName::GlobalPlainText => {
                    self.auth_step(Step::Read);
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
        let mut prompt = PasswordPrompt::new(
            &attempt.target.definition.name,
            attempt.target.definition.summary(),
            save_allowed && attempt.target.connection.is_some(),
        );
        prompt.plain = attempt.no_vault;
        prompt.refused = attempt.target.refused;
        self.auth.prompt = Some(prompt);
    }

    /// Asks before a password leaves the plain text file. "Not now" uses it once more.
    fn ask_migration(&mut self, password: Secret) {
        let Some(attempt) = &mut self.auth.active else {
            return;
        };
        attempt.target.set_password(Some(password));
        let token = attempt.token;
        self.show_dialog(
            text::TITLE_MIGRATION,
            text::MIGRATION_QUESTION.to_owned(),
            vec![
                (text::OPTION_SESSION, DialogAction::AuthSession(token)),
                (text::OPTION_MIGRATE, DialogAction::AuthMigrate(token)),
            ],
        );
    }

    fn auth_vault_error(&mut self, error: VaultError) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        let token = attempt.token;
        let writing = matches!(attempt.step, Step::Write | Step::WritePlain);
        let offer_plain = matches!(attempt.step, Step::Write) && error.no_vault();
        let note = match (offer_plain, writing) {
            (true, _) => format!("\n{}", text::AUTH_PLAIN_WARNING),
            (false, true) => format!("\n{}", text::AUTH_SAVE_FAILED),
            (false, false) => String::new(),
        };
        let mut options = vec![
            (text::OPTION_CANCEL, DialogAction::AuthCancel(token)),
            (text::OPTION_RETRY, DialogAction::AuthRetry(token)),
            (text::OPTION_SESSION, DialogAction::AuthSession(token)),
        ];
        // The vault failed, so plain text needs its own choice after the warning.
        if offer_plain {
            options.push((text::OPTION_SAVE_PLAIN, DialogAction::AuthPlain(token)));
        }
        self.show_dialog(text::TITLE_VAULT, format!("{error}{note}"), options);
    }

    fn persist_auth_reference(&mut self) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        let token = attempt.token;
        let plain = matches!(attempt.step, Step::WritePlain);
        let wanted = if plain { CredentialName::GlobalPlainText } else { CredentialName::Vault };
        // A project cannot name the plain text file, so its reference stays as it is.
        let keeps_reference = plain && attempt.target.key.scope == Scope::Project;
        if attempt.save_reference && attempt.target.credential != wanted && !keeps_reference {
            let (key, definition) = (
                attempt.target.key.clone(),
                attempt.target.definition.clone(),
            );
            if self
                .save_connection(&key, &definition, wanted)
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
                connection.credential = wanted;
            }
            self.auth.active.as_mut().unwrap().target.credential = wanted;
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
            Use::Catalog(id, read) => {
                manager.load_catalog(id, read);
                // The answer of an older read does not end the state, so it cannot start it.
                if let Some(connection) = self.connection_mut(id).filter(|item| item.catalog_read == read) {
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
                let (first, show) = (request.first, request.show);
                let execution = manager.run(console, vec![request.sql], options);
                tab.started(execution, first, show);
            }
            Use::Test(test) => {
                let Some(definition) = definition else { return };
                manager.test_connection(test, definition.connector(), definition.connect_timeout);
            }
            Use::Save => {}
            Use::NewPassword => self.status = Some(text::AUTH_PASSWORD_ACCEPTED.to_owned()),
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
            Use::Catalog(id, read) => {
                if let Some(connection) = self.connection_mut(*id).filter(|item| item.catalog_read == *read) {
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
            DialogAction::AuthPlain(_) => self.auth_step(Step::WritePlain),
            DialogAction::AuthMigrate(_) => self.auth_step(Step::Migrate),
            DialogAction::AuthRemovePlain(_) => {
                self.auth.active.as_mut().unwrap().save_reference = true;
                self.auth_step(Step::RemovePlain);
            }
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
                if attempt.target.refused {
                    self.verify_password();
                } else {
                    self.keep_password();
                }
            }
        }
    }

    fn keep_password(&mut self) {
        let Some(attempt) = &self.auth.active else {
            return;
        };
        match (attempt.save_reference, attempt.no_vault) {
            (true, true) => self.auth_step(Step::WritePlain),
            (true, false) => self.auth_step(Step::Write),
            (false, _) => self.cache_auth_session(),
        }
    }

    fn is_refused(&self, id: ConnectionId) -> bool {
        let generation = self.connection(id).map(|connection| connection.generation);
        self.auth.refused.get(&id).is_some_and(|item| Some(item.generation) == generation)
    }

    /// One wrong password can fail several sessions, so only the first event counts.
    pub(super) fn on_password_refused(&mut self, id: ConnectionId, pool: u64) {
        let current = self.manager.as_ref().and_then(|manager| manager.pool_serial(id)) == Some(pool);
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
        self.auth.refused.insert(id, Refusal { generation, count: 1 });
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
    pub(super) fn ask_new_password(&mut self, id: ConnectionId) -> bool {
        let refused = self.is_refused(id);
        if refused {
            self.authorize(id, Use::NewPassword);
        }
        refused
    }

    /// A refused password never reaches a store, so the check comes before the save.
    fn verify_password(&mut self) {
        let test = self.next_test;
        self.next_test += 1;
        let Some(attempt) = &mut self.auth.active else {
            return;
        };
        attempt.step = Step::Verify;
        attempt.verify = Some(test);
        let definition = attempt.target.definition.clone();
        if let Some(manager) = &self.manager {
            manager.test_connection(test, definition.connector(), definition.connect_timeout);
        }
    }

    /// Takes the answer of `verify_password`. It returns false for a test of the form.
    pub(super) fn on_password_verified(&mut self, test: u64, result: &Result<(), ConnectError>) -> bool {
        let Some(attempt) = self.auth.active.as_mut().filter(|item| item.verify == Some(test)) else {
            return false;
        };
        attempt.verify = None;
        let id = attempt.target.connection;
        match result {
            Ok(()) => {
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
    pub(super) fn delete_password(&mut self, identity: Identity) {
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

impl DialogAction {
    fn auth_token(&self) -> Option<u64> {
        match self {
            Self::AuthCancel(token)
            | Self::AuthConfirm(token)
            | Self::AuthRetry(token)
            | Self::AuthSession(token)
            | Self::AuthPlain(token)
            | Self::AuthMigrate(token)
            | Self::AuthRemovePlain(token) => Some(*token),
            _ => None,
        }
    }
}
