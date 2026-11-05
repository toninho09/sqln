//! Application coordination for authorized local assistants.

mod artifact;
mod jobs;
mod read;
mod review;

use super::{App, DialogAction, Picker, PickerKind};
use crate::agent::ipc::{Envelope, Server};
use crate::agent::{self, Error, Operation, Reply, Text};
use crate::connector::TransactionState;
use crate::console::Phase;
use crate::editor::RunPlan;
use crate::exec::{ConnectionId, ConsoleId, ExecutionId, RunOptions};
use crate::store::config::AgentSettings;
use jobs::task_kind;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub(super) struct State {
    pub server: Option<Server>,
    /// The MCP preferences of the configuration.
    pub settings: AgentSettings,
    /// The folder of the endpoint, or the reason that it has none. `None`: this
    /// application has no endpoint.
    pub directory: Option<Result<PathBuf, String>>,
    sessions: HashMap<String, Session>,
    jobs: HashMap<String, Job>,
    proposals: HashMap<String, Proposal>,
    direct: HashSet<ConsoleId>,
    owned: HashMap<ConsoleId, String>,
    /// The access level of each client that the user allowed, until SQLN exits.
    remembered: HashMap<String, Level>,
    /// The access requests, proposals, and tasks that the user put off with **Later** or
    /// **Hide**. The MCP row keeps them.
    later: HashSet<String>,
    /// The revision of the SQL that the agent placed in its tab. A changed revision means
    /// that the user edited the SQL, so the next SQL needs a review.
    placed: HashMap<ConsoleId, u64>,
    /// Agent tabs with placed SQL that the user did not see yet.
    fresh: HashSet<ConsoleId>,
    retired: HashMap<ConsoleId, CancellationToken>,
    pub review: Option<crate::ui::agent::Review>,
    sender: mpsc::Sender<Work>,
    receiver: mpsc::Receiver<Work>,
    serial: u64,
}
impl State {
    pub(super) fn has_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }
    pub(super) fn has_proposals(&self) -> bool {
        self.pending_proposals() != 0
    }
    pub(super) fn pending_proposals(&self) -> usize {
        self.proposals
            .values()
            .filter(|proposal| proposal.state == "pending")
            .count()
    }
    /// Counts the tasks that run or wait for the user, for all sessions.
    pub(super) fn active_tasks(&self) -> usize {
        self.jobs
            .values()
            .filter(|job| !finished(job.state))
            .count()
    }
    pub(super) fn direct(&self, console: ConsoleId) -> bool {
        self.direct.contains(&console)
    }
    pub(super) fn pending_access(&self) -> usize {
        self.sessions
            .values()
            .filter(|session| session.state == "pending" && !session.alive.is_cancelled())
            .count()
    }
    /// Lists the waiting sessions, the oldest first.
    fn pending_session_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.state == "pending" && !session.alive.is_cancelled())
            .map(|(id, session)| (session.requested, id.clone()))
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    }
    /// Lists the proposals that wait for review, the oldest first.
    fn pending_proposal_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self
            .proposals
            .iter()
            .filter(|(_, proposal)| proposal.state == "pending")
            .map(|(id, proposal)| (proposal.created, id.clone()))
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    }
    /// Returns the item of the card, with its place and the count of its kind: the oldest
    /// access request, then the oldest proposal, then the oldest long task.
    pub(super) fn notice(&self) -> Option<(Notice, usize, usize)> {
        let first = |ids: Vec<String>, kind: fn(String) -> Notice| {
            let total = ids.len();
            ids.into_iter()
                .enumerate()
                .find(|(_, id)| !self.later.contains(id))
                .map(|(index, id)| (kind(id), index + 1, total))
        };
        first(self.pending_session_ids(), Notice::Access)
            .or_else(|| first(self.pending_proposal_ids(), Notice::Proposal))
            .or_else(|| first(self.long_task_ids(), Notice::Task))
    }
    /// Lists the tasks that run for longer than the card delay, the oldest first.
    fn long_task_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self
            .jobs
            .iter()
            .filter(|(_, job)| !finished(job.state) && job.created.elapsed() >= TASK_CARD_DELAY)
            .map(|(id, job)| (job.serial, id.clone()))
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    }
    /// Hides the card of an item. The MCP row keeps it.
    pub(super) fn put_off(&mut self, id: &str) {
        self.later.insert(id.to_owned());
    }
    /// Lists the tasks of a session that did not end, the oldest first.
    fn session_task_ids(&self, session: &str) -> Vec<String> {
        let mut ids: Vec<_> = self
            .jobs
            .iter()
            .filter(|(_, job)| job.session == session && !finished(job.state))
            .map(|(id, job)| (job.serial, id.clone()))
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    }
    fn session_tasks(&self, session: &str) -> usize {
        self.jobs
            .values()
            .filter(|job| job.session == session && !finished(job.state))
            .count()
    }
    /// Lists the sessions with access.
    fn allowed_ids(&self) -> impl Iterator<Item = String> + '_ {
        self.sessions
            .iter()
            .filter(|(_, session)| session.state == "authorized" && !session.alive.is_cancelled())
            .map(|(id, _)| id.clone())
    }
    /// Moves the agent marks of a console to its new identifier, when it takes a connection.
    pub(super) fn rekey_console(&mut self, old: ConsoleId, new: ConsoleId) {
        if let Some(owner) = self.owned.remove(&old) {
            self.owned.insert(new, owner);
        }
        if let Some(revision) = self.placed.remove(&old) {
            self.placed.insert(new, revision);
        }
        if self.fresh.remove(&old) {
            self.fresh.insert(new);
        }
        if self.direct.remove(&old) {
            self.direct.insert(new);
        }
    }
    fn allowed(&self) -> impl Iterator<Item = &Session> {
        self.sessions
            .values()
            .filter(|session| session.state == "authorized" && !session.alive.is_cancelled())
    }
    /// Returns the status bar label while a session has access, such as `MCP 1 full`.
    pub(super) fn status_label(&self) -> Option<String> {
        let count = self.allowed().count();
        let level = self.allowed().map(|session| session.level).max()?;
        Some(crate::ui::text::agent_status(count, level.mark()))
    }
    /// Returns the access level of an authorized session.
    fn level(&self, session: &str) -> Option<Level> {
        self.sessions
            .get(session)
            .filter(|session| session.state == "authorized")
            .map(|session| session.level)
    }
    /// Returns the client name of a session, without terminal control sequences.
    pub(super) fn client(&self, session: &str) -> String {
        self.sessions.get(session).map_or_else(
            || "MCP".into(),
            |session| crate::grid::printable(&session.client),
        )
    }
    pub(super) fn console_label(&self, console: ConsoleId, name: &str) -> String {
        let mut label = name.to_owned();
        if let Some(owner) = self.owned.get(&console)
            && self
                .sessions
                .get(owner)
                .is_none_or(|session| session.state != "authorized")
        {
            label = crate::ui::text::agent_inactive_label(&label);
        }
        if self.direct.contains(&console) {
            label = format!("{}{label}", crate::ui::text::DIRECT_PREFIX);
        }
        if self.fresh.contains(&console) {
            label = format!("{}{label}", crate::ui::text::SQL_PREFIX);
        }
        label
    }
    /// Returns the live session of an agent tab. A disconnected session has none.
    pub(super) fn tab_session(&self, console: ConsoleId) -> Option<&str> {
        let owner = self.owned.get(&console)?;
        self.sessions
            .get(owner)
            .filter(|session| !session.alive.is_cancelled())
            .map(|_| owner.as_str())
    }
    /// Returns the label of the access button of an agent tab, such as `Access: Full access`.
    pub(super) fn tab_access(&self, console: ConsoleId) -> Option<String> {
        let session = self.tab_session(console)?;
        Some(crate::ui::text::agent_access_button(
            self.level(session).map(Level::choice),
        ))
    }
    /// Counts the active tasks of the session of an agent tab.
    pub(super) fn tab_tasks(&self, console: ConsoleId) -> usize {
        self.tab_session(console)
            .map_or(0, |session| self.session_tasks(session))
    }
    pub(super) fn owns(&self, console: ConsoleId) -> bool {
        self.owned.contains_key(&console)
    }
    pub(super) fn task_retired(&self, console: ConsoleId) -> bool {
        self.retired.contains_key(&console)
    }
    pub(super) fn release_retired(&mut self, console: ConsoleId) {
        self.retired.remove(&console);
    }
    pub(super) fn forget_console(&mut self, console: ConsoleId) {
        self.direct.remove(&console);
        self.owned.remove(&console);
        self.placed.remove(&console);
        self.fresh.remove(&console);
        self.retired.remove(&console);
    }
}
impl Default for State {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            server: None,
            settings: AgentSettings::default(),
            directory: None,
            sessions: HashMap::new(),
            jobs: HashMap::new(),
            proposals: HashMap::new(),
            direct: HashSet::new(),
            owned: HashMap::new(),
            remembered: HashMap::new(),
            later: HashSet::new(),
            placed: HashMap::new(),
            fresh: HashSet::new(),
            retired: HashMap::new(),
            review: None,
            sender,
            receiver,
            serial: 0,
        }
    }
}
struct Session {
    client: String,
    state: &'static str,
    alive: CancellationToken,
    requested: Instant,
    level: Level,
}

/// What an authorized session can do. Each level includes the previous levels.
pub(super) use crate::store::config::AgentAccess as Level;

/// The names of a level in the protocol and in the interface.
trait LevelText {
    fn protocol_name(self) -> &'static str;
    /// The name of the option that gives this level.
    fn choice(self) -> &'static str;
    /// The mark of the status bar label.
    fn mark(self) -> Option<&'static str>;
}
impl LevelText for Level {
    fn protocol_name(self) -> &'static str {
        match self {
            Self::Propose => "propose",
            Self::Direct => "direct",
            Self::Full => "full",
        }
    }
    fn choice(self) -> &'static str {
        crate::ui::text::agent_access_name(self)
    }
    fn mark(self) -> Option<&'static str> {
        match self {
            Self::Propose => None,
            Self::Direct => Some("all"),
            Self::Full => Some("full"),
        }
    }
}
struct Job {
    session: String,
    state: &'static str,
    message: Option<String>,
    cancel: CancellationToken,
    consoles: Vec<ConsoleId>,
    connections: Vec<(ConnectionId, u64)>,
    executions: Vec<(ConsoleId, ExecutionId)>,
    ends: HashMap<ConsoleId, crate::exec::ExecutionEnd>,
    draft: Option<Draft>,
    output: Output,
    artifact: Option<Text>,
    results: Vec<Value>,
    /// The end of each executed statement, for the reply.
    statements: Vec<Value>,
    /// The rows of each result in the reply.
    rows: usize,
    serial: u64,
    created: Instant,
    _hold: Option<agent::Hold>,
}
struct Draft {
    plan: RunPlan,
    options: RunOptions,
    revision: Option<u64>,
    explain: Option<crate::plan::Explain>,
    internal: bool,
    /// The plan holds one raw text. The validation worker splits it with this dialect.
    split: Option<crate::sql::Dialect>,
    /// False only for a session with full access.
    read_only: bool,
    /// True after the user confirms a risky run of a session with full access.
    confirmed: bool,
}
enum Output {
    Query,
    Ddl(crate::definition::Target, String, crate::sql::Dialect),
    Diagram(ConsoleId),
    Compare(ConsoleId),
    Catalog(ConnectionId, u64),
}
struct Proposal {
    session: String,
    console: ConsoleId,
    revision: u64,
    edits: Vec<agent::Edit>,
    state: &'static str,
    diff: Option<Text>,
    /// The added and the removed lines of the diff.
    lines: (usize, usize),
    created: Instant,
    _hold: Option<agent::Hold>,
}
enum Work {
    /// Holds the split statements when the draft held one raw text.
    Validated(String, Result<Option<Vec<String>>, Error>),
    Diff(String, Result<Text, Error>),
    Artifact(String, Result<Text, Error>),
}

/// The item of the agent card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    Access(String),
    Proposal(String),
    Task(String),
}

/// A task shows its card after this time, so a short query does not flash on screen.
pub(super) const TASK_CARD_DELAY: Duration = Duration::from_secs(3);

/// The answer to an access request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grant {
    Reject,
    Allow,
    /// Access and direct edits in every console, for this session.
    All,
    /// Also database writes, for this session.
    Full,
}
impl Grant {
    fn level(self) -> Option<Level> {
        match self {
            Self::Reject => None,
            Self::Allow => Some(Level::Propose),
            Self::All => Some(Level::Direct),
            Self::Full => Some(Level::Full),
        }
    }
}

#[derive(Clone)]
pub(super) enum Choice {
    Session(String),
    Proposal(String),
    /// A client with remembered access and no live session.
    Remembered(String),
    /// An agent task that did not end.
    Task(String),
}

fn id(text: &str) -> Result<u64, Error> {
    text.parse().map_err(|_| {
        Error::new(
            "invalid_target",
            "Read the available targets and use an exact identifier.",
        )
    })
}
fn missing() -> Error {
    Error::new(
        "invalid_target",
        "The target is no longer available. Read its list again.",
    )
}
fn revision_conflict() -> Error {
    Error::new(
        "revision_conflict",
        "The console changed. Read it again before editing or executing.",
    )
}
fn supported() -> Error {
    Error::new(
        "unsupported",
        "This operation is not available for this source or object. Check its capabilities.",
    )
}

impl App {
    /// Enables the private local endpoint. Tests and embedded callers can supply a private directory.
    /// With MCP off in the settings, the endpoint opens when the user turns it on.
    pub fn start_agent_at(&mut self, directory: PathBuf) -> std::io::Result<String> {
        if self.agent.server.is_some() {
            return Err(std::io::Error::other(
                "The assistant endpoint already runs.",
            ));
        }
        self.agent.directory = Some(Ok(directory.clone()));
        if !self.agent.settings.enabled {
            return Err(std::io::Error::other(crate::ui::text::AGENT_MCP_OFF));
        }
        let project = self
            .persist
            .as_ref()
            .and_then(|persist| persist.store.project.as_ref())
            .map(|root| root.base.clone());
        let server = Server::at(directory, project)?;
        let id = server.instance.instance_id.clone();
        self.agent.server = Some(server);
        Ok(id)
    }

    /// Opens the endpoint when MCP is on and the application has a folder for it.
    /// A failure goes to the status bar.
    pub(super) fn open_agent_endpoint(&mut self) {
        if !self.agent.settings.enabled || self.agent.server.is_some() {
            return;
        }
        let failure = match self.agent.directory.clone() {
            None => return,
            Some(Err(reason)) => reason,
            Some(Ok(directory)) => match self.start_agent_at(directory) {
                Ok(_) => return,
                Err(error) => crate::ui::text::agent_endpoint(&error),
            },
        };
        self.status = Some(failure);
    }

    /// Applies the MCP preferences of a load at once. MCP off revokes every session and
    /// closes the endpoint. A lower maximum lowers the sessions and the remembered
    /// clients above it.
    pub(super) fn apply_agent_settings(&mut self, settings: AgentSettings) {
        self.agent.settings = settings;
        if !settings.enabled {
            if self.agent.server.is_none() {
                return;
            }
            let live: Vec<_> = self
                .agent
                .sessions
                .iter()
                .filter(|(_, session)| {
                    matches!(session.state, "pending" | "authorized")
                        && !session.alive.is_cancelled()
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in live {
                self.agent_revoke(&id);
            }
            self.agent.remembered.clear();
            self.agent.server = None;
            self.status = Some(crate::ui::text::agent_mcp_closed());
            return;
        }
        if !settings.remember_clients {
            self.agent.remembered.clear();
        }
        let max = settings.max_access;
        for level in self.agent.remembered.values_mut() {
            *level = (*level).min(max);
        }
        let lowered: Vec<_> = self
            .agent
            .sessions
            .iter_mut()
            .filter(|(_, session)| session.state == "authorized" && session.level > max)
            .map(|(id, session)| {
                session.level = max;
                id.clone()
            })
            .collect();
        for id in lowered {
            let notice =
                crate::ui::text::agent_access_lowered(&self.agent.client(&id), max.choice());
            self.agent_notice(&id, None, notice);
        }
        self.refresh_tree_consoles();
        self.open_agent_endpoint();
    }

    pub(super) fn drain_agent(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_millis(2);
        let ended: Vec<_> = self
            .agent
            .sessions
            .iter()
            .filter(|(_, session)| session.alive.is_cancelled() && session.state != "disconnected")
            .map(|(id, _)| id.clone())
            .collect();
        let mut changed = !ended.is_empty();
        for session in ended {
            self.end_agent_session(&session, "disconnected");
        }
        for _ in 0..8 {
            if Instant::now() >= deadline {
                break;
            }
            if let Ok(work) = self.agent.receiver.try_recv() {
                self.on_agent_work(work);
                changed = true;
                continue;
            }
            let request = self
                .agent
                .server
                .as_ref()
                .and_then(|server| server.requests.try_recv().ok());
            let Some(request) = request else { break };
            if request.alive.is_cancelled() || request.reply.is_closed() {
                continue;
            }
            let result = self.agent_request(&request);
            let result = match &result {
                Ok(value)
                    if serde_json::to_vec(value)
                        .map_or(true, |bytes| bytes.len() > agent::PAGE_BYTES) =>
                {
                    Err(Error::new(
                        "response_too_large",
                        "Use a smaller result page or read individual cells.",
                    ))
                }
                _ => result,
            };
            let _ = request.reply.send(result);
            changed = true;
        }
        // A put-off item that ended leaves the list.
        if !self.agent.later.is_empty() {
            let agent = &mut self.agent;
            let sessions = &agent.sessions;
            let (proposals, jobs) = (&agent.proposals, &agent.jobs);
            agent.later.retain(|id| {
                sessions.get(id).is_some_and(|item| item.state == "pending")
                    || proposals
                        .get(id)
                        .is_some_and(|item| item.state == "pending")
                    || jobs.get(id).is_some_and(|job| !finished(job.state))
            });
        }
        // Revisions can change through keys, a save conflict, or another assistant.
        let mut discarded = Vec::new();
        for proposal in self.agent.proposals.values_mut() {
            if matches!(proposal.state, "pending" | "building")
                && self
                    .consoles
                    .iter()
                    .find(|console| console.id == proposal.console)
                    .is_none_or(|console| console.editor.revision() != proposal.revision)
            {
                proposal.state = "revision_conflict";
                proposal.diff = None;
                proposal.edits.clear();
                proposal._hold = None;
                discarded.push((proposal.session.clone(), proposal.console));
            }
        }
        // One note for each session. A closed console uses the last agent tab of its session.
        discarded.sort();
        discarded.dedup_by(|a, b| a.0 == b.0);
        let any = !discarded.is_empty();
        for (session, console) in discarded {
            let connection = self.console_connection(console);
            self.agent_notice(
                &session,
                connection,
                crate::ui::text::AGENT_DISCARDED_REVISION.into(),
            );
        }
        if any {
            // An open review of that proposal could accept nothing, so it closes.
            if self.agent.review.as_ref().is_some_and(|review| {
                self.agent
                    .proposals
                    .get(&review.proposal)
                    .is_none_or(|proposal| proposal.state != "pending")
            }) {
                self.agent.review = None;
            }
        }
        changed || any
    }

    fn agent_request(&mut self, request: &Envelope) -> Reply {
        use Operation::*;
        let session = &request.session;
        if matches!(request.operation, RequestAccess {}) {
            if !self.agent.sessions.contains_key(session) {
                self.agent
                    .sessions
                    .retain(|_, session| !session.alive.is_cancelled());
                if self.agent.sessions.len() >= 64 {
                    return Err(Error::new(
                        "ui_busy",
                        "Too many assistant sessions are pending. Review them in SQLN.",
                    ));
                }
                // A remembered client keeps the choice of the user. Other clients get the
                // default access. Both stay at or below the maximum.
                let settings = self.agent.settings;
                let remembered = self.agent.remembered.get(&request.client).copied();
                let default = settings.default_access.access();
                let granted = remembered
                    .or(default)
                    .map(|level| level.min(settings.max_access));
                self.agent.sessions.insert(
                    session.clone(),
                    Session {
                        client: request.client.clone(),
                        state: if granted.is_some() {
                            "authorized"
                        } else {
                            "pending"
                        },
                        alive: request.alive.clone(),
                        requested: Instant::now(),
                        level: granted.unwrap_or(Level::Propose),
                    },
                );
                if let Some(level) = granted {
                    self.ensure_agent_tabs();
                    let client = self.agent.client(session);
                    let notice = if remembered.is_some() {
                        crate::ui::text::agent_remembered(&client, level.choice())
                    } else {
                        crate::ui::text::agent_default_access(&client, level.choice())
                    };
                    self.agent_notice(session, None, notice);
                    self.refresh_tree_consoles();
                } else {
                    // A new request rings once, so the user sees it in another window.
                    self.bell = true;
                }
            }
            return Ok(self.agent_access(session));
        }
        let Some(level) = self
            .agent
            .level(session)
            .filter(|_| !request.alive.is_cancelled())
        else {
            return Err(Error::new(
                "access_required",
                "Call request_access and ask the user to approve this session in SQLN.",
            ));
        };
        if !request.operation.is_inspection()
            && (self.modal_is_open() || self.exiting || self.config_blocked())
        {
            return Err(Error::new(
                "ui_busy",
                "SQLN has an open dialog or blocked configuration. Resolve it before starting this operation.",
            ));
        }
        match &request.operation {
            RequestAccess {} => unreachable!(),
            GetContext { offset, limit } => self.agent_context(session, level, *offset, *limit),
            ReadConsole {
                console_id,
                offset,
                revision,
                messages,
                message_offset,
            } => {
                let console = id(console_id)?;
                let mut page = self.agent_read_console(console, *offset, *revision)?;
                if *messages {
                    page["messages"] = self.agent_messages(console, *message_offset, 0)?;
                }
                Ok(page)
            }
            ReadResult {
                tab_id,
                result,
                offset,
                limit,
                generation,
                row,
                column,
            } => match (row, column) {
                (None, None) => {
                    self.agent_result(tab_id, *result, *offset, *limit, generation.as_deref())
                }
                (Some(row), Some(column)) => {
                    let generation = generation.as_deref().ok_or_else(|| {
                        Error::new(
                            "generation_required",
                            "Use the generation from the result page.",
                        )
                    })?;
                    self.agent_cell(tab_id, *result, *row, *column, generation, *offset)
                }
                _ => Err(Error::new(
                    "invalid_request",
                    "Send both row and column to read a cell, or neither to read rows.",
                )),
            },
            Query {
                connection_id,
                schema,
                sql,
                parameters,
                explain,
                rows,
            } => {
                let console = self.agent_console(session, id(connection_id)?, schema.clone())?;
                self.agent_query(session, console, sql, parameters, *explain, rows_of(*rows))
            }
            RunConsole {
                console_id,
                revision,
                range,
                explain,
                rows,
            } => self.agent_run_console(
                session,
                id(console_id)?,
                *revision,
                range.as_ref(),
                *explain,
                rows_of(*rows),
            ),
            EditConsole {
                console_id: Some(console_id),
                revision,
                edits,
                mode,
                ..
            } => {
                let revision = revision.ok_or_else(|| {
                    Error::new(
                        "invalid_request",
                        "Send the revision from read_console with the edits.",
                    )
                })?;
                if edits.is_empty() {
                    return Err(Error::new(
                        "invalid_request",
                        "Send at least one edit, or omit console_id to open a new console.",
                    ));
                }
                self.agent_edit(session, id(console_id)?, revision, edits, *mode)
            }
            EditConsole {
                console_id: None,
                connection_id,
                schema,
                sql,
                ..
            } => {
                let (Some(connection), Some(sql)) = (connection_id, sql) else {
                    return Err(Error::new(
                        "invalid_request",
                        "Send console_id to edit a console, or connection_id and sql to open one.",
                    ));
                };
                self.agent_place(session, id(connection)?, schema.clone(), sql)
            }
            Describe {
                connection_id,
                schema,
                object,
                er_diagram,
                refresh,
            } => {
                let connection = id(connection_id)?;
                let schema = || {
                    schema.as_deref().ok_or_else(|| {
                        Error::new(
                            "schema_required",
                            "Send the schema. Read the schema names with describe and no object.",
                        )
                    })
                };
                if *er_diagram {
                    let table = object.as_ref().map(|object| object.name.as_str());
                    self.agent_diagram(session, connection, schema()?, table)
                } else if let Some(object) = object {
                    self.agent_ddl(
                        session,
                        connection,
                        schema()?,
                        &object.name,
                        object.kind.as_str(),
                        object.identity.as_deref(),
                    )
                } else {
                    self.agent_catalog(session, connection, *refresh)
                }
            }
            Compare {
                kind,
                source,
                target,
                removals,
                rows,
            } => {
                let kind = match kind {
                    agent::CompareKind::Structure => crate::compare::Kind::Structure,
                    agent::CompareKind::Data => crate::compare::Kind::Data,
                };
                self.agent_compare(session, source, target, *removals, kind, rows_of(*rows))
            }
            Job {
                job_id,
                cancel,
                artifact_offset,
                rows,
            } => self.agent_job(session, job_id, *cancel, *artifact_offset, *rows),
        }
    }

    /// Reads the application state that an agent needs to choose a target.
    fn agent_context(&self, session: &str, level: Level, offset: usize, limit: usize) -> Reply {
        let connections = self.agent_connections(0, CONTEXT_CONNECTIONS)?;
        let tabs = self.agent_tabs(offset, limit)?;
        let proposals: Vec<_> = self
            .agent
            .pending_proposal_ids()
            .into_iter()
            .filter(|id| self.agent.proposals[id].session == session)
            .collect();
        Ok(json!({
            "session_id": session,
            "access": level.protocol_name(),
            "database_access": if level == Level::Full { "read_write" } else { "read_only" },
            "interface_busy": self.modal_is_open(),
            "active_tab": tabs["active_tab"],
            "connections": connections["connections"],
            "connections_total": connections["total"],
            "tabs": tabs["tabs"],
            "next_offset": tabs["next_offset"],
            "tabs_total": tabs["total"],
            "pending_proposals": proposals,
        }))
    }

    fn agent_access(&self, session: &str) -> Value {
        let item = self.agent.sessions.get(session);
        json!({
            "session_id": session,
            "status": item.map_or("not_requested", |session| session.state),
            "access": item.filter(|item| item.state == "authorized").map(|item| item.level.protocol_name()),
        })
    }

    fn agent_target(&self, console: ConsoleId) -> Result<usize, Error> {
        let index = self.console_index(console).ok_or_else(missing)?;
        let console = &self.consoles[index];
        if console.hidden {
            return Err(missing());
        }
        if console.transaction != TransactionState::None {
            return Err(Error::new(
                "transaction_active",
                "The console has an open, failed, or unknown transaction. Use the agent console.",
            ));
        }
        if console.is_busy() || self.association_pending(console.id) {
            return Err(Error::new(
                "console_busy",
                "The console has pending work. Wait or use the agent console.",
            ));
        }
        if console.connection.is_none() {
            return Err(Error::new(
                "no_connection",
                "The user must attach a connection before this console can execute.",
            ));
        }
        Ok(index)
    }

    fn agent_console(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: Option<String>,
    ) -> Result<ConsoleId, Error> {
        let dialect = self.connection(connection).ok_or_else(missing)?.dialect;
        if let Some(index) = self.consoles.iter().position(|console| {
            self.agent
                .owned
                .get(&console.id)
                .is_some_and(|owner| owner == session)
                && console.connection == Some(connection)
        }) {
            if schema.is_some() && self.consoles[index].schema != schema {
                self.agent_target(self.consoles[index].id)?;
                self.consoles[index].schema = schema;
            }
            return Ok(self.consoles[index].id);
        }
        // The session tab that opened at the access decision takes its first connection.
        let index = match self.consoles.iter().position(|console| {
            console.connection.is_none()
                && self
                    .agent
                    .owned
                    .get(&console.id)
                    .is_some_and(|owner| owner == session)
        }) {
            Some(index) => index,
            None => {
                self.open_agent_tab(session);
                self.consoles.len() - 1
            }
        };
        self.bind_console(index, connection);
        let console = &mut self.consoles[index];
        if console.connection != Some(connection) {
            return Err(missing());
        }
        console.editor.set_dialect(dialect);
        console.schema = schema;
        Ok(console.id)
    }

    /// Opens the agent tab of a session without a connection, after the tab row's other
    /// tabs. The focus and the active tab stay.
    pub(super) fn open_agent_tab(&mut self, session: &str) {
        let mut console = self.unbound_console(self.fallback_dialect);
        console.name =
            crate::ui::text::agent_console_name(&self.agent.client(session), short(session));
        console.focus = crate::console::Focus::Results;
        self.agent.owned.insert(console.id, session.to_owned());
        self.consoles.push(console);
    }

    /// Keeps one agent tab for each session with access. A session without a tab gets one.
    pub(super) fn ensure_agent_tabs(&mut self) {
        if self.exiting {
            return;
        }
        let sessions: Vec<_> = self
            .agent
            .allowed_ids()
            .filter(|session| !self.agent.owned.values().any(|owner| owner == session))
            .collect();
        for session in sessions {
            self.open_agent_tab(&session);
        }
    }

    /// Returns true when closing this agent tab leaves its connected session without a tab.
    pub(super) fn last_agent_tab(&self, console: ConsoleId) -> Option<String> {
        let owner = self.agent.owned.get(&console)?;
        self.agent.level(owner)?;
        let tabs = self
            .agent
            .owned
            .values()
            .filter(|item| *item == owner)
            .count();
        (tabs == 1 && self.agent.tab_session(console).is_some()).then(|| owner.clone())
    }

    /// Places SQL in the editor of the session's agent tab for the user. Nothing executes.
    /// SQL that the user changed stays: the new SQL then waits for a review.
    fn agent_place(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: Option<String>,
        sql: &str,
    ) -> Reply {
        if sql.trim().is_empty() {
            return Err(Error::new(
                "invalid_request",
                "Send the SQL that the user must review.",
            ));
        }
        if sql.len() > agent::MAX_FRAME {
            return Err(Error::new(
                "request_too_large",
                "Send at most 8 MiB of SQL.",
            ));
        }
        let namespace = self
            .connection(connection)
            .ok_or_else(missing)?
            .capabilities
            .namespace
            != crate::connector::Namespace::None;
        let console = self.agent_console(session, connection, None)?;
        let index = self.console_index(console).ok_or_else(missing)?;
        // A running task keeps the schema of its session.
        if namespace
            && schema.is_some()
            && !self.consoles[index].is_busy()
            && !self.consoles[index].in_transaction()
        {
            self.consoles[index].schema = schema;
        }
        let text = sql.replace("\r\n", "\n").replace('\r', "\n");
        let editor = &self.consoles[index].editor;
        let revision = editor.revision();
        let edit = editor.agent_replace_all(&text);
        if !editor.is_blank() && self.agent.placed.get(&console) != Some(&revision) {
            return self.agent_edit(
                session,
                console,
                revision,
                &[edit],
                agent::EditMode::Propose,
            );
        }
        let editor = &mut self.consoles[index].editor;
        editor.apply_agent_edits(revision, &[edit])?;
        // New SQL shows from its first line.
        editor.set_cursor(edtui::Index2::new(0, 0));
        editor.set_scroll(0, 0);
        self.agent_sql_placed(console);
        let client = self.agent.client(session);
        let run = self.shown_key(crate::input::Action::Run);
        self.consoles[index].messages.note(
            crate::console::MessageLevel::Info,
            &crate::ui::text::agent_placed_note(&client, run.as_deref()),
            None,
        );
        self.status = Some(crate::ui::text::agent_placed(&client));
        Ok(json!({
            "console_id": console.to_string(),
            "revision": self.consoles[index].editor.revision(),
            "status": "placed",
        }))
    }

    /// Records SQL that the agent placed in its tab, and marks the tab until the user sees it.
    pub(super) fn agent_sql_placed(&mut self, console: ConsoleId) {
        let Some(index) = self.console_index(console) else {
            return;
        };
        self.agent
            .placed
            .insert(console, self.consoles[index].editor.revision());
        if !(index == self.active && self.console_on_screen()) {
            self.agent.fresh.insert(console);
        }
        self.refresh_tree_consoles();
    }

    /// Removes the mark of SQL that the user did not see, when the agent tab is on screen.
    pub(super) fn see_agent_sql(&mut self) {
        let id = self.console().id;
        if self.console_on_screen() && self.agent.fresh.remove(&id) {
            self.refresh_tree_consoles();
        }
    }

    /// Copies the SQL of the agent tab to a new user console, which takes the focus.
    pub(super) fn agent_sql_to_console(&mut self) {
        let console = self.console();
        let Some(connection) = console.connection else {
            return;
        };
        let (schema, text) = (console.schema.clone(), console.editor.text());
        self.open_text_console(connection, schema, &text);
    }

    /// Empties the SQL of the agent tab as one undo step. The tab then shows its results only.
    pub(super) fn agent_sql_clear(&mut self) {
        let editor = &self.console().editor;
        let (revision, edit) = (editor.revision(), editor.agent_replace_all(""));
        if self
            .console_mut()
            .editor
            .apply_agent_edits(revision, &[edit])
            .is_ok()
        {
            let undo = self.shown_key(crate::input::Action::EditorUndo);
            self.status = Some(crate::ui::text::agent_sql_cleared(undo.as_deref()));
        }
    }

    fn agent_check_job(&self, session: &str, id: &str) -> Result<(), Error> {
        self.agent
            .jobs
            .get(id)
            .filter(|job| job.session == session)
            .map(|_| ())
            .ok_or_else(missing)
    }
    /// Reads a task or an edit proposal, and cancels a task on request.
    fn agent_job(
        &mut self,
        session: &str,
        id: &str,
        cancel: bool,
        artifact_offset: usize,
        rows: Option<usize>,
    ) -> Reply {
        if self
            .agent
            .proposals
            .get(id)
            .is_some_and(|proposal| proposal.session == session)
        {
            if cancel {
                return Err(Error::new(
                    "invalid_request",
                    "Only the user decides an edit proposal in SQLN.",
                ));
            }
            return self.agent_proposal_reply(id);
        }
        self.agent_check_job(session, id)?;
        if cancel {
            self.cancel_agent_job(id);
        }
        if let Some(rows) = rows {
            self.agent.jobs.get_mut(id).expect("checked job").rows = rows_of(Some(rows));
        }
        self.agent_job_page(session, id, artifact_offset)
    }

    fn agent_proposal_reply(&self, id: &str) -> Reply {
        let proposal = self.agent.proposals.get(id).ok_or_else(missing)?;
        let revision = self
            .console_index(proposal.console)
            .map(|index| self.consoles[index].editor.revision());
        Ok(json!({
            "job_id": id,
            "kind": "proposal",
            "status": proposal.state,
            "console_id": proposal.console.to_string(),
            "revision": proposal.revision,
            "console_revision": revision,
        }))
    }

    fn agent_job_reply(&self, session: &str, id: &str) -> Reply {
        self.agent_job_page(session, id, 0)
    }
    /// Writes the task state. A completed task also holds its first result pages and
    /// its first artifact page, within the response limit.
    fn agent_job_page(&self, session: &str, id: &str, artifact_offset: usize) -> Reply {
        self.agent_check_job(session, id)?;
        let job = &self.agent.jobs[id];
        let mut results = Vec::new();
        let mut artifact = Value::Null;
        if job.state == "completed" {
            let inline = matches!(job.output, Output::Query | Output::Compare(_));
            let mut bytes = 0_usize;
            // The pages share half of the response limit. The other half keeps the rest.
            for reference in job.results.iter().filter(|_| inline) {
                let page = (job.rows != 0)
                    .then(|| {
                        self.agent_result(
                            reference["tab_id"].as_str().unwrap_or_default(),
                            reference["result"].as_u64().unwrap_or_default() as usize,
                            0,
                            job.rows,
                            reference["generation"].as_str(),
                        )
                        .ok()
                    })
                    .flatten()
                    .map(|page| {
                        let size = serde_json::to_vec(&page).map_or(usize::MAX, |page| page.len());
                        (page, size)
                    })
                    .filter(|(_, size)| bytes.saturating_add(*size) <= agent::PAGE_BYTES / 2);
                match page {
                    Some((page, size)) => {
                        bytes += size;
                        results.push(page);
                    }
                    None => {
                        let mut item = reference.clone();
                        item["rows_omitted"] = json!(true);
                        results.push(item);
                    }
                }
            }
            if let Some(text) = &job.artifact {
                artifact = agent::text_page(&text.value, artifact_offset)?;
            }
        }
        Ok(json!({
            "job_id": id,
            "kind": task_kind(&job.output),
            "status": job.state,
            "message": job.message,
            "tab_ids": job.consoles.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "statements": job.statements,
            "results": results,
            "artifact": artifact,
            "artifact_format": if matches!(job.output, Output::Catalog(..) | Output::Diagram(_)) { "json" } else { "text" },
        }))
    }

    fn end_agent_session(&mut self, session: &str, state: &'static str) {
        if let Some(item) = self.agent.sessions.get_mut(session) {
            item.state = state;
        }
        self.agent.later.remove(session);
        let jobs: Vec<_> = self
            .agent
            .jobs
            .iter()
            .filter(|(_, job)| job.session == session && !finished(job.state))
            .map(|(id, _)| id.clone())
            .collect();
        for job in jobs {
            self.cancel_agent_job(&job);
        }
        let removed: Vec<_> = self
            .agent
            .jobs
            .iter()
            .filter(|(_, job)| job.session == session)
            .map(|(id, _)| id.clone())
            .collect();
        for id in removed {
            self.agent.jobs.remove(&id);
        }
        if self.agent.review.as_ref().is_some_and(|review| {
            self.agent
                .proposals
                .get(&review.proposal)
                .is_some_and(|proposal| proposal.session == session)
        }) {
            self.agent.review = None;
        }
        self.agent
            .proposals
            .retain(|_, proposal| proposal.session != session);
        self.refresh_tree_consoles();
    }
}
/// Returns the first eight characters of an identifier, as the interface shows it.
pub(super) fn short(id: &str) -> &str {
    &id[..8.min(id.len())]
}
fn finished(state: &str) -> bool {
    matches!(state, "completed" | "failed" | "cancelled")
}
/// The connections that `get_context` lists.
const CONTEXT_CONNECTIONS: usize = 200;
/// The rows of each result in a task reply: 100 by default, at most 1,000.
fn rows_of(rows: Option<usize>) -> usize {
    rows.unwrap_or(100).min(1000)
}
