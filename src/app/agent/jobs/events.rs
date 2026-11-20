//! The progress of agent jobs: execution events, results, and completion.

use super::*;

impl App {
    pub(in crate::app::agent) fn on_agent_work(&mut self, work: Work) {
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
                    Ok((statements, bounded)) => {
                        let Some(job) = self.agent.jobs.get_mut(&id) else {
                            return;
                        };
                        if let Some(statements) = statements
                            && let Some(draft) = &mut job.draft
                        {
                            if draft.split.is_some() {
                                let count = statements.len();
                                draft.plan = RunPlan {
                                    statements,
                                    rows: vec![0; count],
                                    starts: vec![0; count],
                                    ordinals: Vec::new(),
                                };
                            } else {
                                // A console range keeps the places of its statements.
                                draft.plan.statements = statements;
                            }
                        }
                        job.state = "ready";
                        job.default_scan = bounded;
                        if bounded {
                            let session = job.session.clone();
                            let connection = job.connections.first().map(|(id, _)| *id);
                            self.agent_log(
                                &session,
                                connection,
                                &text::agent_default_scan(
                                    crate::connector::kafka::command::AGENT_SCAN,
                                ),
                            );
                        }
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
                        Ok((hunks, hold)) => {
                            proposal.lines = hunks.iter().fold((0, 0), |(added, removed), hunk| {
                                (added + hunk.new.len(), removed + hunk.old)
                            });
                            proposal.hunks = Some(hunks);
                            // The changes replace the edits in the budget.
                            proposal.edits.clear();
                            proposal._hold = Some(hold);
                            proposal.state = "pending";
                            let (session, console) = (proposal.session.clone(), proposal.console);
                            let name = self
                                .console_index(console)
                                .map_or_else(String::new, |index| {
                                    crate::grid::printable(&self.tabs.consoles[index].name)
                                });
                            let notice = text::agent_proposed(&self.agent.client(&session), &name);
                            let connection = self.console_connection(console);
                            self.agent_notice(&session, connection, notice);
                        }
                        Err(_) => {
                            proposal.state = "budget_reached";
                            proposal.edits.clear();
                            proposal._hold = None;
                            let (session, console) = (proposal.session.clone(), proposal.console);
                            let connection = self.console_connection(console);
                            self.agent_notice(
                                &session,
                                connection,
                                text::AGENT_DISCARDED_BUDGET.into(),
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
                ExecEvent::Notice { text, .. } if matches!(job.output, Output::Query) => {
                    keep_notice(&mut job.notices, text);
                }
                ExecEvent::StatementFinished {
                    key, status, end, ..
                } => {
                    if let StatementStatus::Error(message) = status {
                        job.message = Some(message.clone());
                    }
                    let notices = std::mem::take(&mut job.notices);
                    if matches!(job.output, Output::Query)
                        && *status != StatementStatus::NotExecuted
                        && job.statements.len() < STATEMENTS
                    {
                        job.statements.push(statement_value(
                            key.index,
                            status,
                            end.as_ref(),
                            notices,
                        ));
                    }
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

    pub(super) fn capture_agent_results(&mut self, id: &str) {
        let consoles = self.agent.jobs[id].consoles.clone();
        let mut results = Vec::new();
        for console in consoles {
            if let Some(index) = self.console_index(console) {
                results.extend(self.tabs.consoles[index].results.iter().enumerate().map(|(result,grid)|json!({"tab_id":console.to_string(),"result":result,"generation":grid.table.generation.to_string()})));
            }
            if let Some(tab) = self
                .tabs
                .compares
                .iter()
                .find(|tab| tab.sides[0].id == console)
            {
                results.push(json!({"tab_id":console.to_string(),"result":0,"generation":tab.result.table.generation.to_string()}));
            }
        }
        if let Some(item) = self.agent.jobs.get_mut(id) {
            item.results = results;
        }
    }

    pub(super) fn complete_agent_job(&mut self, id: &str, artifact: Option<Text>) {
        let Some(job) = self.agent.jobs.get(id) else {
            return;
        };
        if job.state != "building_artifact" && !job.executions.is_empty() {
            self.capture_agent_results(id);
        }
        let Some(job) = self.agent.jobs.get_mut(id) else {
            return;
        };
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
                    self.tabs.consoles[index].phase = Phase::Idle;
                }
            }
        }
        let kind = task_kind(&self.agent.jobs[id].output);
        self.agent_log(
            &session,
            Some(connection),
            &text::agent_task_note(kind, short(id), "completed", None),
        );
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
                // A metadata read has no execution, so it never waits here.
                Output::Catalog(..) | Output::Fields(_) => Ok(None),
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
                    let Some(tab) = self.tabs.diagrams.iter().find(|item| item.id == *tab) else {
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
                    let Some(tab) = self
                        .tabs
                        .compares
                        .iter()
                        .find(|item| item.sides[0].id == *tab)
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
            };
            match result {
                Ok(Some(work)) => {
                    self.capture_agent_results(&id);
                    if let Some(item) = self.agent.jobs.get_mut(&id) {
                        item.state = "building_artifact";
                    }
                    match self.manager.as_ref() {
                        Some(manager) => manager.spawn_blocking(work),
                        None => {
                            self.fail_agent_job(&id, "The execution manager is not available.");
                        }
                    }
                }
                Ok(None) => {}
                Err(error) => self.fail_agent_job(&id, &error.message),
            }
        }
    }
}
