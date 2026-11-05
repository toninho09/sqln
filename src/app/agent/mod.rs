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
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub(super) struct State {
    pub server: Option<Server>,
    sessions: HashMap<String, Session>,
    jobs: HashMap<String, Job>,
    proposals: HashMap<String, Proposal>,
    direct: HashSet<ConsoleId>,
    owned: HashMap<ConsoleId, String>,
    relations: HashMap<String, crate::catalog::RelationRef>,
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
        self.proposals
            .values()
            .any(|proposal| proposal.state == "pending")
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
    pub(super) fn console_label(&self, console: ConsoleId, name: &str) -> String {
        let mut label = name.to_owned();
        if let Some(owner) = self.owned.get(&console) {
            if self
                .sessions
                .get(owner)
                .is_none_or(|session| session.state != "authorized")
            {
                label = format!("Agent inactive | {label}");
            } else if !name.starts_with("Agent:") {
                label = format!("Agent: {label}");
            }
        }
        if self.direct.contains(&console) {
            label = format!("Direct edits | {label}");
        }
        label
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
        self.retired.remove(&console);
    }
}
impl Default for State {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            server: None,
            sessions: HashMap::new(),
            jobs: HashMap::new(),
            proposals: HashMap::new(),
            direct: HashSet::new(),
            owned: HashMap::new(),
            relations: HashMap::new(),
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
    serial: u64,
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
}
enum Output {
    Query,
    Ddl(crate::definition::Target, String, crate::sql::Dialect),
    Diagram(ConsoleId),
    Compare(ConsoleId),
    Catalog(ConnectionId, u64),
    Generate(crate::generate::Template, String, crate::sql::Dialect),
}
struct Proposal {
    session: String,
    console: ConsoleId,
    revision: u64,
    edits: Vec<agent::Edit>,
    state: &'static str,
    diff: Option<Text>,
    _hold: Option<agent::Hold>,
}
enum Work {
    /// Holds the split statements when the draft held one raw text.
    Validated(String, Result<Option<Vec<String>>, Error>),
    Diff(String, Result<Text, Error>),
    Artifact(String, Result<Text, Error>),
}

#[derive(Clone)]
pub(super) enum Choice {
    Session(String),
    Proposal(String),
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
    pub fn start_agent_at(&mut self, directory: std::path::PathBuf) -> std::io::Result<String> {
        if self.agent.server.is_some() {
            return Err(std::io::Error::other(
                "The assistant endpoint already runs.",
            ));
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
        // Revisions can change through keys, a save conflict, or another assistant.
        let mut discarded = false;
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
                discarded = true;
            }
        }
        if discarded {
            self.status = Some(
                "Agent edit discarded: the console changed or closed. Ask the agent to propose again."
                    .into(),
            );
        }
        changed || discarded
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
                self.agent.sessions.insert(
                    session.clone(),
                    Session {
                        client: request.client.clone(),
                        state: "pending",
                        alive: request.alive.clone(),
                    },
                );
            }
            return Ok(self.agent_access(session));
        }
        if matches!(request.operation, GetAccess {}) {
            return Ok(self.agent_access(session));
        }
        if !self
            .agent
            .sessions
            .get(session)
            .is_some_and(|session| session.state == "authorized" && !session.alive.is_cancelled())
        {
            return Err(Error::new(
                "access_required",
                "Request access and ask the user to approve this session in SQLN.",
            ));
        }
        if !request.operation.is_inspection()
            && (self.modal_is_open() || self.exiting || self.config_blocked())
        {
            return Err(Error::new(
                "ui_busy",
                "SQLN has an open dialog or blocked configuration. Resolve it before starting this operation.",
            ));
        }
        match &request.operation {
            RequestAccess {} | GetAccess {} => unreachable!(),
            GetContext {} => Ok(
                json!({"active_tab": self.agent_active_tab(), "session_id": session, "database_access":"read_only", "edit_default":"propose", "interface_busy":self.modal_is_open()}),
            ),
            ListConnections { offset, limit } => self.agent_connections(*offset, *limit),
            ListTabs { offset, limit } => self.agent_tabs(*offset, *limit),
            ReadConsole {
                console_id,
                offset,
                revision,
            } => self.agent_read_console(id(console_id)?, *offset, *revision),
            ReadMessages {
                console_id,
                offset,
                text_offset,
            } => self.agent_messages(id(console_id)?, *offset, *text_offset),
            ReadResult {
                tab_id,
                result,
                offset,
                limit,
                generation,
            } => self.agent_result(tab_id, *result, *offset, *limit, generation.as_deref()),
            ReadResultCell {
                tab_id,
                result,
                row,
                column,
                generation,
                offset,
            } => self.agent_cell(tab_id, *result, *row, *column, generation, *offset),
            OpenAgentConsole {
                connection_id,
                schema,
            } => {
                let console = self.agent_console(session, id(connection_id)?, schema.clone())?;
                Ok(json!({"console_id":console.to_string()}))
            }
            RunReadQuery {
                console_id,
                sql,
                parameters,
            } => self.agent_query(session, id(console_id)?, sql, parameters),
            RunConsole {
                console_id,
                revision,
                range,
            } => self.agent_run_console(session, id(console_id)?, *revision, range.as_ref(), None),
            Explain {
                console_id,
                revision,
                range,
                analyze,
            } => self.agent_run_console(
                session,
                id(console_id)?,
                *revision,
                range.as_ref(),
                Some(*analyze),
            ),
            GetJob { job_id } => self.agent_job_reply(session, job_id),
            CancelJob { job_id } => {
                self.agent_check_job(session, job_id)?;
                self.cancel_agent_job(job_id);
                self.agent_job_reply(session, job_id)
            }
            EditConsole {
                console_id,
                revision,
                edits,
                mode,
            } => self.agent_edit(session, id(console_id)?, *revision, edits, *mode),
            GetProposal { proposal_id } => {
                let proposal = self
                    .agent
                    .proposals
                    .get(proposal_id)
                    .filter(|proposal| proposal.session == *session)
                    .ok_or_else(missing)?;
                Ok(
                    json!({"proposal_id":proposal_id,"status":proposal.state,"console_id":proposal.console.to_string(),"revision":proposal.revision}),
                )
            }
            GetCatalog {
                connection_id,
                refresh,
            } => self.agent_catalog(session, id(connection_id)?, *refresh),
            GetObjectDdl {
                connection_id,
                schema,
                name,
                kind,
                identity,
            } => self.agent_ddl(
                session,
                id(connection_id)?,
                schema,
                name,
                kind.as_str(),
                identity.as_deref(),
            ),
            GenerateSql {
                connection_id,
                schema,
                table,
                template,
            } => self.agent_generate(
                session,
                id(connection_id)?,
                schema,
                table,
                template.as_str(),
            ),
            GetErDiagram {
                connection_id,
                schema,
                table,
            } => self.agent_diagram(session, id(connection_id)?, schema, table.as_deref()),
            CompareSchema {
                source,
                target,
                removals,
            } => self.agent_compare(
                session,
                source,
                target,
                *removals,
                crate::compare::Kind::Structure,
            ),
            CompareData {
                source,
                target,
                removals,
            } => self.agent_compare(
                session,
                source,
                target,
                *removals,
                crate::compare::Kind::Data,
            ),
            ReadArtifact { job_id, offset } => {
                self.agent_check_job(session, job_id)?;
                let artifact = self
                    .agent
                    .jobs
                    .get(job_id)
                    .and_then(|job| job.artifact.as_ref())
                    .ok_or_else(|| {
                        Error::new(
                            "not_ready",
                            "The text artifact is not available. Check the task state.",
                        )
                    })?;
                agent::text_page(&artifact.value, *offset)
            }
        }
    }

    fn agent_access(&self, session: &str) -> Value {
        json!({"session_id":session,"status":self.agent.sessions.get(session).map_or("not_requested", |session| session.state)})
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
        let mut console = self.unbound_console(dialect);
        let id = self
            .manager
            .as_mut()
            .and_then(|manager| manager.open_console(connection))
            .ok_or_else(missing)?;
        console.id = id;
        console.connection = Some(connection);
        console.schema = schema;
        let client = self
            .agent
            .sessions
            .get(session)
            .map_or("MCP", |session| session.client.as_str());
        console.name = format!(
            "Agent: {} [{}]",
            crate::grid::printable(client),
            &session[..8.min(session.len())]
        );
        self.agent.owned.insert(id, session.to_owned());
        self.consoles.push(console);
        self.refresh_tree_consoles();
        Ok(id)
    }

    fn agent_check_job(&self, session: &str, id: &str) -> Result<(), Error> {
        self.agent
            .jobs
            .get(id)
            .filter(|job| job.session == session)
            .map(|_| ())
            .ok_or_else(missing)
    }
    fn agent_job_reply(&self, session: &str, id: &str) -> Reply {
        self.agent_check_job(session, id)?;
        let job = &self.agent.jobs[id];
        Ok(
            json!({"job_id":id,"status":job.state,"message":job.message,"tab_ids":job.consoles.iter().map(|id|id.to_string()).collect::<Vec<_>>(),"artifact":job.artifact.is_some(),"artifact_format":if matches!(job.output,Output::Catalog(..)|Output::Diagram(_)){"json"}else{"text"},"results":job.results}),
        )
    }

    fn end_agent_session(&mut self, session: &str, state: &'static str) {
        if let Some(item) = self.agent.sessions.get_mut(session) {
            item.state = state;
        }
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
            self.agent.relations.remove(&id);
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
fn finished(state: &str) -> bool {
    matches!(state, "completed" | "failed" | "cancelled")
}
