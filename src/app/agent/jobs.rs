//! Assistant tasks use the existing executor and credential gate.

use super::artifact::{diagram_bytes, diagram_value};
use super::*;
use crate::app::auth::Use;
use crate::catalog::{ObjectKind, RelationRef};
use crate::connector::{Failure, Fix};
use crate::exec::{AgentMetadata, ExecEvent, ExecutionEnd, MetadataRead, StatementStatus};
use crate::sql::{Dialect, split};
use crate::ui::text;
use serde_json::{Value, json};

mod events;
mod reads;

impl State {
    /// Internal catalog SQL keeps the same ownership and read-only session policy.
    pub(in crate::app) fn run(
        &mut self,
        manager: &crate::exec::Manager,
        console: ConsoleId,
        statements: Vec<String>,
        options: RunOptions,
    ) -> ExecutionId {
        if let Some(cancel) = self.retired.get(&console) {
            return manager.run_agent(console, statements, options, cancel.clone(), true, true);
        }
        if let Some(job) = self.jobs.values_mut().find(|job| {
            job.consoles.contains(&console)
                && !finished(job.state)
                && job.state != "building_artifact"
        }) {
            job.state = "running";
            let execution =
                manager.run_agent(console, statements, options, job.cancel.clone(), true, true);
            job.executions.retain(|(id, _)| *id != console);
            job.executions.push((console, execution));
            job.ends.remove(&console);
            execution
        } else {
            manager.run(console, statements, options)
        }
    }
}

impl App {
    fn new_agent_job(
        &mut self,
        session: &str,
        consoles: Vec<ConsoleId>,
        connections: Vec<ConnectionId>,
        draft: Option<Draft>,
        output: Output,
    ) -> Result<String, Error> {
        if self
            .agent
            .jobs
            .values()
            .filter(|job| job.session == session && !finished(job.state))
            .count()
            >= 4
        {
            return Err(Error::new(
                "too_many_jobs",
                "This session already has four active tasks. Wait or cancel one.",
            ));
        }
        if self
            .agent
            .jobs
            .values()
            .filter(|job| job.session == session)
            .count()
            >= 64
            && let Some(old) = self
                .agent
                .jobs
                .iter()
                .filter(|(_, job)| job.session == session && finished(job.state))
                .min_by_key(|(_, job)| job.serial)
                .map(|(id, _)| id.clone())
        {
            self.agent.jobs.remove(&old);
        }
        let connections = connections
            .into_iter()
            .map(|id| {
                self.connection(id)
                    .map(|connection| (id, connection.generation))
                    .ok_or_else(missing)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bytes = draft.as_ref().map_or(0, |draft| {
            draft
                .plan
                .statements
                .iter()
                .map(String::capacity)
                .sum::<usize>()
        });
        let hold = agent::Hold::new(bytes.saturating_mul(2), &self.budget)?;
        let notification_connection = connections[0].0;
        let notice = self.agent_console(session, notification_connection, None)?;
        if let Some(index) = self.console_index(notice) {
            let target = self.tabs.consoles[index].messages.id;
            let histories: Vec<_> = consoles
                .iter()
                .zip(&connections)
                .filter(|(console, _)| self.console_index(**console).is_none())
                .map(|(console, (connection, _))| {
                    (
                        *console,
                        self.connection(*connection).map_or_else(
                            || "Agent".into(),
                            |connection| connection.plain_name().to_owned(),
                        ),
                    )
                })
                .collect();
            if let Some(manager) = &mut self.manager {
                for (console, name) in histories {
                    manager.set_console_history(console, target, name);
                }
            }
            self.tabs.consoles[index].messages.note(
                crate::console::MessageLevel::Info,
                "Agent task queued. Use its task identifier to inspect or cancel it.",
                None,
            );
        }
        for console in &consoles {
            if let Some(index) = self.console_index(*console) {
                self.tabs.consoles[index].phase = Phase::Waiting;
            }
        }
        let cancel = self
            .agent
            .sessions
            .get(session)
            .ok_or_else(missing)?
            .alive
            .child_token();
        let id = uuid::Uuid::new_v4().to_string();
        self.agent.serial += 1;
        self.agent.jobs.insert(
            id.clone(),
            Job {
                session: session.into(),
                state: "ready",
                message: None,
                fix: None,
                cancel,
                consoles,
                connections,
                executions: Vec::new(),
                ends: HashMap::new(),
                draft,
                output,
                artifact: None,
                results: Vec::new(),
                statements: Vec::new(),
                notices: Vec::new(),
                default_scan: None,
                rows: rows_of(None),
                serial: self.agent.serial,
                created: Instant::now(),
                _hold: Some(hold),
            },
        );
        Ok(id)
    }

    fn queue_agent_draft(
        &mut self,
        session: &str,
        console: ConsoleId,
        draft: Draft,
        output: Output,
        rows: usize,
    ) -> Reply {
        let index = self.agent_target(console)?;
        let connection = self.tabs.consoles[index].connection.ok_or_else(missing)?;
        let item = self.connection(connection).ok_or_else(missing)?;
        let dialect = item.dialect;
        // The connection value wins over Max scan of the settings.
        let scan = item
            .definition
            .as_ref()
            .map_or(self.settings.max_scan, |definition| {
                definition.settings(&self.settings).max_scan
            });
        let total = draft.plan.statements.iter().map(String::len).sum::<usize>();
        if total > agent::MAX_FRAME {
            return Err(Error::new(
                "request_too_large",
                "Execute a smaller script range, at most 8 MiB.",
            ));
        }
        let internal = draft.internal;
        let read_only = draft.read_only;
        let raw = draft.split;
        // The split keeps a second copy of the text while it runs.
        let reserved = match (internal, raw) {
            (true, _) => 0,
            (false, None) => total,
            (false, Some(_)) => total.saturating_mul(2),
        };
        let validation_hold = agent::Hold::new(reserved, &self.budget)?;
        let statements = (!internal).then(|| draft.plan.statements.clone());
        let job = self.new_agent_job(
            session,
            vec![console],
            vec![connection],
            Some(draft),
            output,
        )?;
        if let Some(item) = self.agent.jobs.get_mut(&job) {
            item.rows = rows;
        }
        if let Some(statements) = statements {
            if let Some(item) = self.agent.jobs.get_mut(&job) {
                item.state = "validating";
            }
            let sender = self.agent.sender.clone();
            let key = job.clone();
            self.manager
                .as_ref()
                .ok_or_else(missing)?
                .spawn_blocking(move || {
                    let _hold = validation_hold;
                    let statements = match raw {
                        Some(text_dialect) => statements.first().map_or_else(Vec::new, |sql| {
                            split(sql, text_dialect)
                                .iter()
                                .map(|part| part.text(sql).to_owned())
                                .collect()
                        }),
                        None => statements,
                    };
                    // Full access keeps the split and leaves the checks to the database.
                    let checked = if read_only {
                        agent::policy::validate(&statements, dialect)
                    } else {
                        Ok(())
                    };
                    // A read without a bound gets one, at each access level.
                    let result = checked.map(|()| {
                        let features = crate::dialect::features(dialect);
                        let mut bounded = false;
                        let statements: Vec<_> = statements
                            .into_iter()
                            .map(|sql| match features.default_bound(&sql, scan) {
                                Some(sql) => {
                                    bounded = true;
                                    sql
                                }
                                None => sql,
                            })
                            .collect();
                        let scan = bounded.then_some(scan);
                        ((raw.is_some() || bounded).then_some(statements), scan)
                    });
                    let _ = sender.send(Work::Validated(key, result));
                });
        }
        self.agent_job_reply(session, &job)
    }

    pub(in crate::app) fn agent_query(
        &mut self,
        session: &str,
        console: ConsoleId,
        sql: &str,
        parameters: &[agent::Binding],
        explain: Option<agent::ExplainMode>,
        rows: usize,
    ) -> Reply {
        if self
            .agent
            .owned
            .get(&console)
            .is_none_or(|owner| owner != session)
        {
            return Err(Error::new(
                "own_console_required",
                "New SQL can run only in this session's agent console. User consoles execute their current editor text.",
            ));
        }
        let index = self.agent_target(console)?;
        let dialect = self.tabs.consoles[index].editor.dialect();
        let parameters = bindings(parameters)?;
        let explain = explain
            .map(|mode| {
                crate::plan::Explain::new(dialect, mode == agent::ExplainMode::Analyze)
                    .ok_or_else(supported)
            })
            .transpose()?;
        let options = RunOptions {
            schema: self.tabs.consoles[index].schema.clone(),
            parameters,
            ..Default::default()
        };
        // The validation worker splits the text, so a large script does not block a frame.
        let plan = RunPlan {
            statements: vec![sql.to_owned()],
            rows: vec![0],
            starts: vec![0],
            ordinals: Vec::new(),
        };
        self.queue_agent_draft(
            session,
            console,
            Draft {
                plan,
                options,
                revision: None,
                explain,
                internal: false,
                split: Some(dialect),
                read_only: self.agent.level(session) != Some(Level::Full),
                confirmed: false,
            },
            Output::Query,
            rows,
        )
    }

    pub(in crate::app) fn agent_run_console(
        &mut self,
        session: &str,
        console: ConsoleId,
        revision: u64,
        range: Option<&agent::Range>,
        explain: Option<agent::ExplainMode>,
        rows: usize,
    ) -> Reply {
        let index = self.agent_target(console)?;
        if self.tabs.consoles[index].editor.revision() != revision {
            return Err(revision_conflict());
        }
        let plan = self.tabs.consoles[index].editor.agent_plan(range)?;
        let dialect = self.tabs.consoles[index].editor.dialect();
        let parameters = self.tabs.consoles[index]
            .parameters
            .values(&plan.statements, dialect)
            .map_err(|message| Error::new("parameters_required", &message))?;
        let explain = if let Some(mode) = explain {
            if plan.statements.len() != 1 {
                return Err(Error::new(
                    "one_statement_required",
                    "Explain accepts exactly one statement.",
                ));
            }
            Some(
                crate::plan::Explain::new(dialect, mode == agent::ExplainMode::Analyze)
                    .ok_or_else(supported)?,
            )
        } else {
            None
        };
        let options = RunOptions {
            schema: self.tabs.consoles[index].schema.clone(),
            parameters,
            ..Default::default()
        };
        self.queue_agent_draft(
            session,
            console,
            Draft {
                plan,
                options,
                revision: Some(revision),
                explain,
                internal: false,
                split: None,
                read_only: self.agent.level(session) != Some(Level::Full),
                confirmed: false,
            },
            Output::Query,
            rows,
        )
    }

    pub(in crate::app) fn continue_agent_jobs(&mut self) {
        if self.modal_is_open() || self.exiting {
            return;
        }
        let next = self
            .agent
            .jobs
            .iter()
            .find(|(_, job)| job.state == "ready")
            .map(|(id, _)| id.clone());
        let Some(id) = next else { return };
        let (console, connection) = {
            let job = &self.agent.jobs[&id];
            (job.consoles[0], job.connections[0].0)
        };
        // A write of a session with full access asks the questions of a user run.
        let question = self.agent.jobs[&id]
            .draft
            .as_ref()
            .filter(|draft| !draft.read_only && !draft.confirmed)
            .and_then(|draft| self.run_warning(connection, &draft.plan));
        if let Some(body) = question {
            let session = self.agent.jobs[&id].session.clone();
            if let Some(item) = self.agent.jobs.get_mut(&id) {
                item.state = "waiting_confirmation";
            }
            self.show_dialog(
                text::TITLE_CONFIRM_RUN,
                text::agent_run_question(&self.agent.client(&session), &body),
                vec![
                    (
                        text::OPTION_CANCEL,
                        DialogAction::AgentRun(id.clone(), false),
                    ),
                    (text::OPTION_RUN, DialogAction::AgentRun(id, true)),
                ],
            );
            return;
        }
        if let Some(job) = self.agent.jobs.get_mut(&id) {
            job.state = "waiting_credentials";
        }
        self.authorize(connection, Use::Agent { console, job: id });
    }

    /// Applies the user's answer to the run question of a session with full access.
    pub(in crate::app) fn agent_run_decision(&mut self, id: &str, run: bool) {
        let Some(job) = self
            .agent
            .jobs
            .get_mut(id)
            .filter(|job| job.state == "waiting_confirmation")
        else {
            return;
        };
        if !run {
            self.terminate_agent_job(id, "cancelled", text::AGENT_RUN_REFUSED);
            return;
        }
        if let Some(draft) = &mut job.draft {
            draft.confirmed = true;
        }
        job.state = "ready";
    }

    /// The credential response must still match the original target and session.
    pub(in crate::app) fn dispatch_agent_job(&mut self, id: &str) {
        let Some(job) = self.agent.jobs.get(id) else {
            return;
        };
        let valid = !job.cancel.is_cancelled()
            && job.connections.iter().all(|(id, generation)| {
                self.connection(*id)
                    .is_some_and(|connection| connection.generation == *generation)
            });
        if !valid {
            self.fail_agent_job(
                id,
                "The target or session changed. Submit the operation again.",
            );
            return;
        }
        let console = job.consoles[0];
        if let Some(draft) = &job.draft
            && let Some(index) = self.console_index(console)
            && (self.tabs.consoles[index].transaction != TransactionState::None
                || draft.revision.is_some_and(|revision| {
                    self.tabs.consoles[index].editor.revision() != revision
                }))
        {
            self.fail_agent_job(
                id,
                "The console revision or transaction changed. Read it again before executing.",
            );
            return;
        }
        let Some(manager) = self.manager.as_ref() else {
            self.fail_agent_job(id, "The execution manager is not available.");
            return;
        };
        let Some(job) = self.agent.jobs.get_mut(id) else {
            return;
        };
        job.state = "running";
        if let Some(draft) = job.draft.take() {
            let statements = draft.explain.as_ref().map_or_else(
                || draft.plan.statements.clone(),
                |explain| {
                    draft
                        .plan
                        .statements
                        .iter()
                        .map(|sql| explain.wrap(sql))
                        .collect()
                },
            );
            let execution = manager.run_agent(
                console,
                statements,
                draft.options.clone(),
                job.cancel.clone(),
                draft.internal || draft.explain.is_some(),
                draft.read_only,
            );
            job.executions.push((console, execution));
            let session = job.session.clone();
            if let Some(index) = self.console_index(console) {
                if !self.agent.owns(console) {
                    let notice = text::agent_ran(
                        &self.agent.client(&session),
                        &crate::grid::printable(&self.tabs.consoles[index].name),
                        draft.read_only,
                    );
                    let connection = self.tabs.consoles[index].connection;
                    self.agent_notice(&session, connection, notice);
                }
                self.tabs.consoles[index]
                    .ran_parameters
                    .clone_from(&draft.options.parameters);
                self.tabs.consoles[index].begin_execution(execution, &draft.plan, &self.budget);
                self.tabs.consoles[index].explain = draft.explain;
                if draft.revision.is_none() {
                    self.tabs.consoles[index].editor.clear_marks();
                }
            } else {
                self.fail_agent_job(id, "The console closed before the operation started.");
            }
        } else {
            let connection = job.connections[0].0;
            let request = match &job.output {
                Output::Fields(topic) => MetadataRead::Fields(topic.clone()),
                _ => MetadataRead::Catalog(
                    self.connections
                        .iter()
                        .find(|item| item.id == connection)
                        .and_then(|item| item.definition.as_ref())
                        .map(|definition| definition.schemas.clone())
                        .unwrap_or_default(),
                ),
            };
            manager.agent_metadata(
                console,
                connection,
                id.to_owned(),
                request,
                job.cancel.clone(),
            );
        }
    }

    pub(in crate::app) fn cancel_agent_job(&mut self, id: &str) {
        self.terminate_agent_job(id, "cancelled", "The assistant operation was cancelled.");
    }
    pub(in crate::app) fn fail_agent_job(&mut self, id: &str, message: &str) {
        self.terminate_agent_job(id, "failed", message);
    }

    /// Fails a job with a failure that can have a configuration fix.
    pub(in crate::app) fn fail_agent_job_with(&mut self, id: &str, failure: &Failure) {
        let open = self
            .agent
            .jobs
            .get(id)
            .is_some_and(|job| !finished(job.state));
        self.fail_agent_job(id, &failure.message);
        if let Some(job) = self.agent.jobs.get_mut(id).filter(|_| open) {
            job.fix = failure.fix;
        }
    }
    pub(super) fn terminate_agent_job(&mut self, id: &str, state: &'static str, message: &str) {
        let Some(job) = self.agent.jobs.get_mut(id) else {
            return;
        };
        if finished(job.state) {
            return;
        }
        let building = job.state == "building_artifact";
        let pending = job.executions.is_empty() || job.ends.len() == job.executions.len();
        let consoles = job.consoles.clone();
        let session = job.session.clone();
        let connection = job.connections[0].0;
        job.state = state;
        job.message = Some(message.into());
        // A fix of an earlier statement does not describe this end.
        job.fix = None;
        job.cancel.cancel();
        let cancel = job.cancel.clone();
        job.draft = None;
        job._hold = None;
        if !building {
            for console in consoles {
                self.agent.retired.insert(console, cancel.clone());
                self.cancel_agent_auth_console(console);
                if let Some(index) = self.console_index(console) {
                    self.tabs.consoles[index].phase = if pending {
                        Phase::Idle
                    } else {
                        Phase::Stopping
                    };
                }
            }
        }
        let kind = task_kind(&self.agent.jobs[id].output);
        self.agent_log(
            &session,
            Some(connection),
            &text::agent_task_note(kind, short(id), state, Some(message)),
        );
        if state == "failed" {
            self.status = Some(text::agent_failed(&self.agent.client(&session), kind));
        }
    }

    pub(in crate::app) fn cancel_agent_console(&mut self, console: ConsoleId) -> bool {
        let jobs: Vec<_> = self
            .agent
            .jobs
            .iter()
            .filter(|(_, job)| job.consoles.contains(&console) && !finished(job.state))
            .map(|(id, _)| id.clone())
            .collect();
        let cancelled = !jobs.is_empty();
        for job in jobs {
            self.cancel_agent_job(&job);
        }
        cancelled
    }

    /// Writes a note in the Messages of the session's agent tab for a connection. Without
    /// that tab, it uses the last agent tab of the session. No tab opens for a note.
    pub(super) fn agent_log(
        &mut self,
        session: &str,
        connection: Option<ConnectionId>,
        message: &str,
    ) {
        let owned = |console: &crate::console::Console| {
            self.agent
                .owned
                .get(&console.id)
                .is_some_and(|owner| owner == session)
        };
        let index = connection
            .and_then(|connection| {
                self.tabs
                    .consoles
                    .iter()
                    .position(|console| console.connection == Some(connection) && owned(console))
            })
            .or_else(|| self.tabs.consoles.iter().rposition(owned));
        if let Some(index) = index {
            self.tabs.consoles[index].messages.note(
                crate::console::MessageLevel::Info,
                message,
                None,
            );
        }
    }

    /// Shows a notice of an agent action in the status bar and in the session's Messages.
    pub(super) fn agent_notice(
        &mut self,
        session: &str,
        connection: Option<ConnectionId>,
        message: String,
    ) {
        self.agent_log(session, connection, &message);
        self.status = Some(message);
    }

    /// Returns the connection of a console, for the notices of its agent actions.
    pub(super) fn console_connection(&self, console: ConsoleId) -> Option<ConnectionId> {
        self.console_index(console)
            .and_then(|index| self.tabs.consoles[index].connection)
    }

    /// Closes the executor consoles of an agent read that did not start.
    fn close_agent_consoles(&mut self, consoles: &[ConsoleId]) {
        if let Some(manager) = &mut self.manager {
            for console in consoles {
                manager.close_console(*console);
            }
        }
    }
}

/// Names the kind of a task in notices.
pub(super) fn task_kind(output: &Output) -> &'static str {
    match output {
        Output::Query => "Query",
        Output::Ddl(..) => "DDL read",
        Output::Diagram(_) => "Diagram",
        Output::Compare(_) => "Comparison",
        Output::Catalog(..) => "Catalog read",
        Output::Fields(_) => "Field read",
    }
}

/// The statement ends that a task keeps for its reply.
const STATEMENTS: usize = 100;
/// The notices that one statement keeps for the reply, and their bytes.
const NOTICES: usize = 10;
const NOTICE_BYTES: usize = 2048;

/// Keeps a database notice for the end of its statement, within `NOTICES` and
/// `NOTICE_BYTES`. The text goes only to the agent, never to a diagnostic log.
pub(super) fn keep_notice(notices: &mut Vec<String>, text: &str) {
    let used: usize = notices.iter().map(String::len).sum();
    if notices.len() >= NOTICES || used >= NOTICE_BYTES {
        return;
    }
    let text = crate::grid::printable(text);
    let mut end = text.len().min(NOTICE_BYTES - used);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    notices.push(text[..end].to_owned());
}

/// Writes the end of one statement: its state, its rows or changed rows, the limit
/// that stopped its read, and its notices.
fn statement_value(
    index: usize,
    status: &StatementStatus,
    end: Option<&crate::connector::StatementEnd>,
    notices: Vec<String>,
    fix: Option<Fix>,
) -> Value {
    let state = match status {
        StatementStatus::Running => "running",
        StatementStatus::Success => "success",
        StatementStatus::Error(_) => "error",
        StatementStatus::NotExecuted => "not_executed",
        StatementStatus::Stopped => "stopped",
        StatementStatus::Timeout => "timeout",
        StatementStatus::Unknown => "unknown",
    };
    let mut value = json!({"index": index, "status": state});
    if let StatementStatus::Error(message) = status {
        value["error"] = json!(crate::grid::printable(message));
    }
    if let Some(fix) = fix {
        value["fix"] = json!(fix.hint());
    }
    if let Some(end) = end {
        if end.has_columns {
            value["rows"] = json!(end.rows);
        }
        if let Some(changed) = end.changed {
            value["changed_rows"] = json!(changed);
        }
        // The grid kept only part of the rows: the agent must not read them as all rows.
        let limit = match end.limit {
            crate::sink::ReadLimit::None => None,
            crate::sink::ReadLimit::RowCap => Some("row_cap"),
            crate::sink::ReadLimit::Budget => Some("memory_budget"),
            crate::sink::ReadLimit::ValueTooLarge => Some("value_too_large"),
        };
        if let Some(limit) = limit {
            value["limit"] = json!(limit);
        }
    }
    if !notices.is_empty() {
        value["notices"] = json!(notices);
    }
    value
}

fn bindings(values: &[agent::Binding]) -> Result<Vec<crate::sql::parameters::Parameter>, Error> {
    use crate::sql::parameters::{Kind, Parameter};
    let mut names = HashSet::new();
    values
        .iter()
        .map(|value| {
            if !names.insert(&value.name) {
                return Err(Error::new(
                    "invalid_parameter",
                    "Parameter names must be unique.",
                ));
            }
            let kind = match value.kind {
                agent::BindingKind::Null => Kind::Null,
                agent::BindingKind::Boolean => Kind::Boolean,
                agent::BindingKind::Number => Kind::Number,
                agent::BindingKind::Text => Kind::Text,
            };
            if (kind == Kind::Number && !crate::sql::parameters::is_number(&value.value))
                || (kind == Kind::Boolean && !matches!(value.value.as_str(), "true" | "false"))
            {
                return Err(Error::new(
                    "invalid_parameter",
                    "Use a valid decimal number or boolean.",
                ));
            }
            Ok(Parameter {
                name: value.name.clone(),
                kind,
                value: value.value.clone(),
            })
        })
        .collect()
}
