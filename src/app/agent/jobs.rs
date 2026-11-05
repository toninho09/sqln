//! Assistant tasks use the existing executor and credential gate.

use super::artifact::{diagram_bytes, diagram_value};
use super::*;
use crate::app::auth::Use;
use crate::catalog::{ObjectKind, RelationRef};
use crate::exec::{AgentMetadata, ExecEvent, ExecutionEnd, StatementStatus};
use crate::sql::{Dialect, split};
use serde_json::{Value, json};

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
            return manager.run_agent(console, statements, options, cancel.clone(), true);
        }
        if let Some(job) = self.jobs.values_mut().find(|job| {
            job.consoles.contains(&console)
                && !finished(job.state)
                && job.state != "building_artifact"
        }) {
            job.state = "running";
            let execution =
                manager.run_agent(console, statements, options, job.cancel.clone(), true);
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
            self.agent.relations.remove(&old);
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
            let target = self.consoles[index].messages.id;
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
            self.consoles[index].messages.note(
                crate::console::MessageLevel::Info,
                "Agent task queued. Use its task identifier to inspect or cancel it.",
                None,
            );
        }
        for console in &consoles {
            if let Some(index) = self.console_index(*console) {
                self.consoles[index].phase = Phase::Waiting;
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
                cancel,
                consoles,
                connections,
                executions: Vec::new(),
                ends: HashMap::new(),
                draft,
                output,
                artifact: None,
                results: Vec::new(),
                serial: self.agent.serial,
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
    ) -> Reply {
        let index = self.agent_target(console)?;
        let connection = self.consoles[index].connection.ok_or_else(missing)?;
        let dialect = self.connection(connection).ok_or_else(missing)?.dialect;
        let total = draft.plan.statements.iter().map(String::len).sum::<usize>();
        if total > agent::MAX_FRAME {
            return Err(Error::new(
                "request_too_large",
                "Execute a smaller script range, at most 8 MiB.",
            ));
        }
        let internal = draft.internal;
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
        if let Some(statements) = statements {
            self.agent.jobs.get_mut(&job).expect("new job").state = "validating";
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
                    let result = agent::policy::validate(&statements, dialect)
                        .map(|()| raw.is_some().then_some(statements));
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
        let dialect = self.consoles[index].editor.dialect();
        let parameters = bindings(parameters)?;
        let options = RunOptions {
            schema: self.consoles[index].schema.clone(),
            parameters,
            ..Default::default()
        };
        // The validation worker splits the text, so a large script does not block a frame.
        let plan = RunPlan {
            statements: vec![sql.to_owned()],
            rows: vec![0],
            starts: vec![0],
        };
        self.queue_agent_draft(
            session,
            console,
            Draft {
                plan,
                options,
                revision: None,
                explain: None,
                internal: false,
                split: Some(dialect),
            },
            Output::Query,
        )
    }

    pub(in crate::app) fn agent_run_console(
        &mut self,
        session: &str,
        console: ConsoleId,
        revision: u64,
        range: Option<&agent::Range>,
        analyze: Option<bool>,
    ) -> Reply {
        let index = self.agent_target(console)?;
        if self.consoles[index].editor.revision() != revision {
            return Err(revision_conflict());
        }
        let plan = self.consoles[index].editor.agent_plan(range)?;
        let dialect = self.consoles[index].editor.dialect();
        let parameters = self.consoles[index]
            .parameters
            .values(&plan.statements, dialect)
            .map_err(|message| Error::new("parameters_required", &message))?;
        let explain = if let Some(analyze) = analyze {
            if plan.statements.len() != 1 {
                return Err(Error::new(
                    "one_statement_required",
                    "Explain accepts exactly one statement.",
                ));
            }
            Some(
                crate::plan::Explain::new(self.consoles[index].editor.dialect(), analyze)
                    .ok_or_else(supported)?,
            )
        } else {
            None
        };
        let options = RunOptions {
            schema: self.consoles[index].schema.clone(),
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
            },
            Output::Query,
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
        if let Some(job) = self.agent.jobs.get_mut(&id) {
            job.state = "waiting_credentials";
        }
        self.authorize(connection, Use::Agent { console, job: id });
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
            && (self.consoles[index].transaction != TransactionState::None
                || draft
                    .revision
                    .is_some_and(|revision| self.consoles[index].editor.revision() != revision))
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
        let job = self.agent.jobs.get_mut(id).expect("checked job");
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
            );
            job.executions.push((console, execution));
            if let Some(index) = self.console_index(console) {
                self.consoles[index]
                    .ran_parameters
                    .clone_from(&draft.options.parameters);
                self.consoles[index].begin_execution(execution, &draft.plan, &self.budget);
                self.consoles[index].explain = draft.explain;
                if draft.revision.is_none() {
                    self.consoles[index].editor.clear_marks();
                }
            } else {
                self.fail_agent_job(id, "The console closed before the operation started.");
            }
        } else {
            let connection = job.connections[0].0;
            let relation = self.agent.relations.get(id).cloned();
            let schemas = self
                .connections
                .iter()
                .find(|item| item.id == connection)
                .and_then(|item| item.definition.as_ref())
                .map(|definition| definition.schemas.clone())
                .unwrap_or_default();
            manager.agent_metadata(
                console,
                connection,
                id.to_owned(),
                relation,
                schemas,
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
    fn terminate_agent_job(&mut self, id: &str, state: &'static str, message: &str) {
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
        job.cancel.cancel();
        let cancel = job.cancel.clone();
        job.draft = None;
        job._hold = None;
        if !building {
            for console in consoles {
                self.agent.retired.insert(console, cancel.clone());
                self.cancel_agent_auth_console(console);
                if let Some(index) = self.console_index(console) {
                    self.consoles[index].phase = if pending {
                        Phase::Idle
                    } else {
                        Phase::Stopping
                    };
                }
            }
        }
        self.notify_agent(
            &session,
            connection,
            &format!("Agent task {id} {state}: {message}"),
        );
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

    pub(super) fn on_agent_work(&mut self, work: Work) {
        match work {
            Work::Artifact(id, result) => {
                if self
                    .agent
                    .jobs
                    .get(&id)
                    .is_none_or(|job| finished(job.state) || job.cancel.is_cancelled())
                {
                    return;
                }
                match result {
                    Ok(text) => self.complete_agent_job(&id, Some(text)),
                    Err(error) => self.fail_agent_job(&id, &error.message),
                }
            }
            Work::Validated(id, result) => {
                if self
                    .agent
                    .jobs
                    .get(&id)
                    .is_none_or(|job| job.cancel.is_cancelled())
                {
                    return;
                }
                match result {
                    Ok(statements) => {
                        let job = self.agent.jobs.get_mut(&id).expect("checked job");
                        if let Some(statements) = statements
                            && let Some(draft) = &mut job.draft
                        {
                            let count = statements.len();
                            draft.plan = RunPlan {
                                statements,
                                rows: vec![0; count],
                                starts: vec![0; count],
                            };
                        }
                        job.state = "ready";
                    }
                    Err(error) => self.fail_agent_job(&id, &error.message),
                }
            }
            Work::Diff(id, diff) => {
                if let Some(proposal) = self
                    .agent
                    .proposals
                    .get_mut(&id)
                    .filter(|proposal| proposal.state == "building")
                {
                    match diff {
                        Ok(text) => {
                            proposal.diff = Some(text);
                            proposal.state = "pending";
                            self.status = Some(
                                "Agent edit proposed. Open Review agent changes in the command palette.".into(),
                            );
                        }
                        Err(_) => {
                            proposal.state = "budget_reached";
                            proposal.edits.clear();
                            proposal._hold = None;
                            self.status = Some(
                                "Agent edit discarded: its diff exceeds the memory budget. Ask the agent for a smaller edit.".into(),
                            );
                        }
                    }
                }
            }
        }
    }

    pub(in crate::app) fn note_agent_event(&mut self, event: &ExecEvent) {
        let Some(console) = event.console() else {
            return;
        };
        let Some(execution) = event.execution() else {
            return;
        };
        for job in self
            .agent
            .jobs
            .values_mut()
            .filter(|job| job.executions.contains(&(console, execution)))
        {
            match event {
                ExecEvent::StatementFinished {
                    status: StatementStatus::Error(message),
                    ..
                } => {
                    job.message = Some(message.clone());
                }
                ExecEvent::ExecutionFinished { end, message, .. } => {
                    job.ends.insert(console, *end);
                    if message.is_some() {
                        job.message.clone_from(message);
                    }
                }
                _ => {}
            }
        }
    }

    fn capture_agent_results(&mut self, id: &str) {
        let consoles = self.agent.jobs[id].consoles.clone();
        let mut results = Vec::new();
        for console in consoles {
            if let Some(index) = self.console_index(console) {
                results.extend(self.consoles[index].results.iter().enumerate().map(|(result,grid)|json!({"tab_id":console.to_string(),"result":result,"generation":grid.table.generation.to_string()})));
            }
            if let Some(tab) = self.compares.iter().find(|tab| tab.sides[0].id == console) {
                results.push(json!({"tab_id":console.to_string(),"result":0,"generation":tab.result.table.generation.to_string()}));
            }
        }
        self.agent.jobs.get_mut(id).expect("completed job").results = results;
    }

    fn complete_agent_job(&mut self, id: &str, artifact: Option<Text>) {
        let Some(job) = self.agent.jobs.get(id) else {
            return;
        };
        if job.state != "building_artifact" && !job.executions.is_empty() {
            self.capture_agent_results(id);
        }
        let job = self.agent.jobs.get_mut(id).expect("checked job");
        let native = job.executions.is_empty();
        job.artifact = artifact;
        job.state = "completed";
        job._hold = None;
        let consoles = job.consoles.clone();
        let session = job.session.clone();
        let connection = job.connections[0].0;
        if native {
            for console in consoles {
                if let Some(index) = self.console_index(console) {
                    self.consoles[index].phase = Phase::Idle;
                }
            }
        }
        self.notify_agent(&session, connection, &format!("Agent task {id} completed."));
    }

    fn notify_agent(&mut self, session: &str, connection: ConnectionId, message: &str) {
        let own = self.consoles.iter().position(|console| {
            console.connection == Some(connection)
                && self
                    .agent
                    .owned
                    .get(&console.id)
                    .is_some_and(|owner| owner == session)
        });
        if let Some(index) = own {
            self.consoles[index]
                .messages
                .note(crate::console::MessageLevel::Info, message, None);
        }
    }

    pub(in crate::app) fn poll_agent_jobs(&mut self) {
        let ids: Vec<_> = self
            .agent
            .jobs
            .iter()
            .filter(|(_, job)| {
                !finished(job.state)
                    && job.state != "building_artifact"
                    && !job.executions.is_empty()
                    && job.ends.len() == job.executions.len()
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let job = &self.agent.jobs[&id];
            if job.ends.values().any(|end| *end != ExecutionEnd::Completed) {
                let message = job
                    .message
                    .clone()
                    .unwrap_or_else(|| "The assistant operation did not complete.".into());
                self.fail_agent_job(&id, &message);
                continue;
            }
            let sender = self.agent.sender.clone();
            let budget = self.budget.clone();
            let key = id.clone();
            let result: Result<Option<Box<dyn FnOnce() + Send>>, Error> = match &job.output {
                Output::Query => {
                    self.complete_agent_job(&id, None);
                    continue;
                }
                Output::Ddl(target, name, dialect) => {
                    let (target, name, dialect) = (target.clone(), name.clone(), *dialect);
                    let console = job.consoles[0];
                    match self.agent_definition(console, dialect, &target, &name) {
                        Ok(text) => self.complete_agent_job(&id, Some(text)),
                        Err(error) => self.fail_agent_job(&id, &error.message),
                    }
                    continue;
                }
                Output::Diagram(tab) => {
                    let Some(tab) = self.diagrams.iter().find(|item| item.id == *tab) else {
                        self.fail_agent_job(&id, "The diagram closed.");
                        continue;
                    };
                    if tab.is_busy() {
                        continue;
                    }
                    agent::Hold::new(diagram_bytes(&tab.model), &budget).map(|hold| {
                        let model = tab.model.clone();
                        let partial = tab.partial;
                        Some(Box::new(move || {
                            let _hold = hold;
                            let text = diagram_value(&model, partial).to_string();
                            let result = Text::new(text, &budget);
                            let _ = sender.send(Work::Artifact(key, result));
                        }) as Box<dyn FnOnce() + Send>)
                    })
                }
                Output::Compare(tab) => {
                    let Some(tab) = self.compares.iter().find(|item| item.sides[0].id == *tab)
                    else {
                        self.fail_agent_job(&id, "The comparison closed.");
                        continue;
                    };
                    if tab.is_busy() {
                        continue;
                    }
                    if !tab.done {
                        let message = tab.message.as_ref().map_or_else(
                            || "The comparison did not complete.".into(),
                            |message| message.text.clone(),
                        );
                        self.fail_agent_job(&id, &message);
                        continue;
                    }
                    agent::Hold::new(tab.script.len(), &budget).map(|hold| {
                        let script = tab.script.clone();
                        Some(Box::new(move || {
                            let _hold = hold;
                            let result = Text::new(script, &budget);
                            let _ = sender.send(Work::Artifact(key, result));
                        }) as Box<dyn FnOnce() + Send>)
                    })
                }
                _ => Ok(None),
            };
            match result {
                Ok(Some(work)) => {
                    self.capture_agent_results(&id);
                    self.agent.jobs.get_mut(&id).expect("checked job").state = "building_artifact";
                    self.manager
                        .as_ref()
                        .expect("active job manager")
                        .spawn_blocking(work);
                }
                Ok(None) => {}
                Err(error) => self.fail_agent_job(&id, &error.message),
            }
        }
    }

    /// The DDL text is small, as in the user viewer, so the interface thread writes it.
    /// The rows leave the grids for the call and then go back, so nothing is copied.
    fn agent_definition(
        &mut self,
        console: ConsoleId,
        dialect: Dialect,
        target: &crate::definition::Target,
        name: &str,
    ) -> Result<Text, Error> {
        let index = self.console_index(console).ok_or_else(missing)?;
        let grids = &mut self.consoles[index].results;
        let rows: Vec<_> = grids
            .iter_mut()
            .map(|grid| std::mem::take(&mut grid.table.rows))
            .collect();
        let text = crate::definition::definition_text(dialect, target, name, &rows);
        for (grid, rows) in grids.iter_mut().zip(rows) {
            grid.table.rows = rows;
        }
        text.map_err(|message| Error::new("definition_failed", message))
            .and_then(|text| Text::new(text, &self.budget))
    }

    /// Closes the executor consoles of an agent read that did not start.
    fn close_agent_consoles(&mut self, consoles: &[ConsoleId]) {
        if let Some(manager) = &mut self.manager {
            for console in consoles {
                manager.close_console(*console);
            }
        }
    }

    pub(in crate::app) fn agent_catalog(
        &mut self,
        session: &str,
        connection: ConnectionId,
        refresh: bool,
    ) -> Reply {
        let item = self.connection(connection).ok_or_else(missing)?;
        if !refresh && let Some(catalog) = &item.catalog {
            let value = catalog_value(catalog);
            let text = serde_json::to_string(&value)
                .map_err(|_| Error::new("encoding_failed", "Cannot encode the cached catalog."))?;
            if text.len() <= agent::PAGE_BYTES / 3 {
                return Ok(value);
            }
            let artifact = Text::new(text, &self.budget)?;
            let job = self.new_agent_job(
                session,
                Vec::new(),
                vec![connection],
                None,
                Output::Catalog(connection, self.catalog_reads),
            )?;
            self.complete_agent_job(&job, Some(artifact));
            return self.agent_job_reply(session, &job);
        }
        let console = self.agent_console(session, connection, None)?;
        self.agent_target(console)?;
        self.catalog_reads += 1;
        let read = self.catalog_reads;
        let job = self.new_agent_job(
            session,
            vec![console],
            vec![connection],
            None,
            Output::Catalog(connection, read),
        )?;
        if let Some(item) = self.connection_mut(connection) {
            item.catalog_read = read;
        }
        self.agent_job_reply(session, &job)
    }

    pub(in crate::app) fn on_agent_metadata(
        &mut self,
        id: &str,
        result: Result<AgentMetadata, String>,
    ) {
        let Some(job) = self.agent.jobs.get(id) else {
            return;
        };
        if finished(job.state) || job.cancel.is_cancelled() {
            return;
        }
        if job.connections.iter().any(|(id, generation)| {
            self.connection(*id)
                .is_none_or(|connection| connection.generation != *generation)
        }) {
            self.fail_agent_job(id, "The connection changed during the metadata read.");
            return;
        }
        let text = match result {
            Ok(AgentMetadata::Catalog(catalog)) => {
                let Output::Catalog(connection, read) = job.output else {
                    self.fail_agent_job(id, "The metadata response has the wrong type.");
                    return;
                };
                let value = catalog_value(&catalog);
                self.on_catalog_loaded(connection, read, Ok(catalog));
                serde_json::to_string(&value).map_err(|_| "Cannot encode the catalog.".to_owned())
            }
            Ok(AgentMetadata::Columns(columns)) => {
                if let Output::Generate(template, qualified, dialect) = &job.output {
                    Ok(crate::generate::generate(
                        *template, *dialect, qualified, &columns,
                    ))
                } else {
                    Err("The metadata response has the wrong type.".into())
                }
            }
            Err(message) => Err(message),
        };
        match text.and_then(|text| Text::new(text, &self.budget).map_err(|error| error.message)) {
            Ok(text) => {
                self.complete_agent_job(id, Some(text));
            }
            Err(message) => self.fail_agent_job(id, &message),
        }
    }

    pub(in crate::app) fn agent_ddl(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: &str,
        name: &str,
        kind: &str,
        identity: Option<&str>,
    ) -> Reply {
        let item = self.connection(connection).ok_or_else(missing)?;
        let target = crate::definition::Target {
            schema: schema.into(),
            name: name.into(),
            detail: identity.unwrap_or("").into(),
            kind: match kind {
                "table" => crate::definition::Kind::Table,
                "view" => crate::definition::Kind::View,
                kind => {
                    crate::definition::Kind::Object(ObjectKind::parse(kind).ok_or_else(supported)?)
                }
            },
        };
        let dialect = item.dialect;
        let label = item.plain_name().to_owned();
        let statements =
            crate::definition::definition_sql(dialect, &target).ok_or_else(supported)?;
        let console = self.agent_console(session, connection, Some(schema.into()))?;
        let count = statements.len();
        self.queue_agent_draft(
            session,
            console,
            Draft {
                plan: RunPlan {
                    statements,
                    rows: vec![0; count],
                    starts: vec![0; count],
                },
                options: RunOptions {
                    schema: Some(schema.into()),
                    only_schema: true,
                    row_cap: Some(usize::MAX),
                    ..Default::default()
                },
                revision: None,
                explain: None,
                internal: true,
                split: None,
            },
            Output::Ddl(target, label, dialect),
        )
    }

    pub(in crate::app) fn agent_generate(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: &str,
        table: &str,
        template: &str,
    ) -> Reply {
        let dialect = self.connection(connection).ok_or_else(missing)?.dialect;
        if dialect == Dialect::Redis {
            return Err(supported());
        }
        let template = match template {
            "select_all" => crate::generate::Template::SelectAll,
            "select_columns" => crate::generate::Template::SelectColumns,
            "insert" => crate::generate::Template::Insert,
            "update" => crate::generate::Template::Update,
            "delete" => crate::generate::Template::Delete,
            _ => return Err(supported()),
        };
        let qualified = crate::generate::qualified_name(Some(schema), table, dialect);
        let console = self.agent_console(session, connection, None)?;
        self.agent_target(console)?;
        let job = self.new_agent_job(
            session,
            vec![console],
            vec![connection],
            None,
            Output::Generate(template, qualified.clone(), dialect),
        )?;
        if template.needs_columns() {
            self.agent.relations.insert(
                job.clone(),
                RelationRef {
                    schema: schema.into(),
                    relation: table.into(),
                },
            );
        } else {
            let text = crate::generate::generate(template, dialect, &qualified, &[]);
            let artifact = Text::new(text, &self.budget)?;
            self.complete_agent_job(&job, Some(artifact));
        }
        self.agent_job_reply(session, &job)
    }

    pub(in crate::app) fn agent_diagram(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: &str,
        table: Option<&str>,
    ) -> Reply {
        let dialect = self.connection(connection).ok_or_else(missing)?.dialect;
        let focus = table.map_or_else(
            || crate::diagram::Focus::Schema(schema.into()),
            |table| {
                crate::diagram::Focus::Table(RelationRef {
                    schema: schema.into(),
                    relation: table.into(),
                })
            },
        );
        let console = self
            .manager
            .as_mut()
            .and_then(|manager| manager.open_console(connection))
            .ok_or_else(missing)?;
        let opened = self.next_tab_place();
        let mut tab = crate::diagram::tab::DiagramTab::new(console, connection, focus, opened);
        let Some(statements) = tab.statements(dialect) else {
            self.close_agent_consoles(&[console]);
            return Err(supported());
        };
        let job = match self.new_agent_job(
            session,
            vec![console],
            vec![connection],
            None,
            Output::Diagram(console),
        ) {
            Ok(job) => job,
            Err(error) => {
                self.close_agent_consoles(&[console]);
                return Err(error);
            }
        };
        tab.phase = Phase::Waiting;
        tab.agent = true;
        self.diagrams.push(tab);
        self.agent.jobs.get_mut(&job).expect("new job").state = "waiting_credentials";
        self.authorize(
            connection,
            Use::Diagram {
                console,
                connection,
                statements,
            },
        );
        self.agent_job_reply(session, &job)
    }

    pub(in crate::app) fn agent_compare(
        &mut self,
        session: &str,
        source: &agent::Side,
        target: &agent::Side,
        removals: bool,
        kind: crate::compare::Kind,
    ) -> Reply {
        let sides = [source, target]
            .map(|side| {
                Ok(crate::compare::Side {
                    connection: id(&side.connection_id)?,
                    schema: side.schema.clone(),
                    table: side.table.clone(),
                })
            })
            .into_iter()
            .collect::<Result<Vec<_>, Error>>()?;
        for side in &sides {
            let item = self.connection(side.connection).ok_or_else(missing)?;
            if item.dialect == Dialect::Redis
                || (kind == crate::compare::Kind::Data && side.table.is_none())
            {
                return Err(supported());
            }
        }
        let mut reads: Vec<crate::compare::tab::SideRead> = Vec::new();
        for side in sides {
            let connection = side.connection;
            let read = self
                .connection(connection)
                .map(|item| (item.dialect, format!("Agent: {}", item.plain_name())))
                .and_then(|(dialect, name)| {
                    let console = self.manager.as_mut()?.open_console(connection)?;
                    Some(crate::compare::tab::SideRead::new(
                        console, side, dialect, name,
                    ))
                });
            let Some(read) = read else {
                let opened: Vec<_> = reads.iter().map(|read| read.id).collect();
                self.close_agent_consoles(&opened);
                return Err(missing());
            };
            reads.push(read);
        }
        let consoles = reads.iter().map(|read| read.id).collect::<Vec<_>>();
        let connections = reads.iter().map(|read| read.side.connection).collect();
        let first = consoles[0];
        let Ok(sides) = reads.try_into() else {
            self.close_agent_consoles(&consoles);
            return Err(missing());
        };
        let opened = self.next_tab_place();
        let mut tab = crate::compare::tab::CompareTab::new(opened, kind, sides, removals);
        let requests = (0..2)
            .map(|side| {
                let read = &tab.sides[side];
                Ok((
                    read.id,
                    read.side.connection,
                    read.side.schema.clone(),
                    tab.statements(side).ok_or_else(supported)?,
                ))
            })
            .collect::<Result<Vec<_>, Error>>();
        let started = requests.and_then(|requests| {
            self.new_agent_job(
                session,
                consoles.clone(),
                connections,
                None,
                Output::Compare(first),
            )
            .map(|job| (job, requests))
        });
        let (job, requests) = match started {
            Ok(started) => started,
            Err(error) => {
                self.close_agent_consoles(&consoles);
                return Err(error);
            }
        };
        tab.agent = true;
        tab.begin(&self.budget);
        self.compares.push(tab);
        self.agent.jobs.get_mut(&job).expect("new job").state = "waiting_credentials";
        for (console, connection, schema, statements) in requests {
            self.authorize(
                connection,
                Use::Compare {
                    console,
                    connection,
                    schema,
                    statements,
                    untimed: false,
                },
            );
        }
        self.agent_job_reply(session, &job)
    }
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
fn catalog_value(catalog: &crate::catalog::Catalog) -> Value {
    json!({"current_schema":catalog.current_schema,"schemas":catalog.schemas.iter().map(|schema|json!({"name":schema.name,"relations":schema.relations.iter().map(|relation|json!({"name":relation.name,"kind":format!("{:?}",relation.kind).to_ascii_lowercase(),"columns":match &relation.columns {crate::catalog::Columns::Loaded(columns)=>Some(columns),_=>None}})).collect::<Vec<_>>(),"objects":schema.objects.iter().map(|object|json!({"name":object.name,"kind":format!("{:?}",object.kind).to_ascii_lowercase(),"detail":object.detail})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}
