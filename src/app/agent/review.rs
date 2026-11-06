//! User-only authorization, revocation, and acceptance of edit proposals.

use super::*;
use crate::agent::{Edit, EditMode};
use crate::editor::review::{Review, Source, diff_hunks};
use crate::grid::printable;
use crate::ui::text;
use serde_json::json;

impl App {
    /// Lists the sessions: the waiting sessions first, then the allowed sessions.
    pub(in crate::app) fn open_agent_sessions(&mut self) {
        let rank = |state: &str| match state {
            "pending" => 0,
            "authorized" => 1,
            _ => 2,
        };
        let mut sessions: Vec<_> = self
            .agent
            .sessions
            .iter()
            .filter(|(_, session)| !session.alive.is_cancelled())
            .collect();
        sessions.sort_by_key(|(_, session)| (rank(session.state), session.requested));
        let live: HashSet<_> = sessions
            .iter()
            .filter(|(_, session)| session.state == "authorized")
            .map(|(_, session)| session.client.as_str())
            .collect();
        let mut remembered: Vec<_> = self
            .agent
            .remembered
            .iter()
            .filter(|(client, _)| !live.contains(client.as_str()))
            .map(|(client, level)| {
                (
                    text::agent_remembered_row(&printable(client), level.choice()),
                    Choice::Remembered(client.clone()),
                )
            })
            .collect();
        remembered.sort_by(|a, b| a.0.cmp(&b.0));
        let mut choices: Vec<_> = sessions
            .into_iter()
            .map(|(id, session)| {
                (
                    text::agent_session_row(
                        &printable(&session.client),
                        short(id),
                        session.state,
                        session.level.choice(),
                        self.agent.session_tasks(id),
                    ),
                    Choice::Session(id.clone()),
                )
            })
            .collect();
        choices.extend(remembered);
        self.agent_picker("Agent sessions", choices);
    }
    pub(in crate::app) fn open_agent_proposals(&mut self) {
        let choices = self
            .agent
            .pending_proposal_ids()
            .into_iter()
            .map(|id| {
                let proposal = &self.agent.proposals[&id];
                (
                    text::agent_proposal_row(
                        &printable(&self.proposal_console_name(proposal.console)),
                        &self.agent.client(&proposal.session),
                        proposal.revision,
                    ),
                    Choice::Proposal(id),
                )
            })
            .collect();
        self.agent_picker("Review agent changes", choices);
    }
    /// Opens the session dialog of the active agent tab: change the access or revoke it.
    pub(in crate::app) fn open_tab_agent_session(&mut self) {
        match self.agent.tab_session(self.console().id).map(str::to_owned) {
            Some(session) => self.open_agent_access(&session),
            None => self.status = Some(text::AGENT_NO_SESSION.into()),
        }
    }
    /// Lists the active tasks of the session of the active agent tab, on all connections.
    pub(in crate::app) fn open_agent_tasks(&mut self) {
        let Some(session) = self.agent.tab_session(self.console().id) else {
            self.status = Some(text::AGENT_NO_SESSION.into());
            return;
        };
        let choices = self
            .agent
            .session_task_ids(session)
            .into_iter()
            .map(|id| {
                let job = &self.agent.jobs[&id];
                (
                    text::agent_task_row(
                        jobs::task_kind(&job.output),
                        short(&id),
                        job.state,
                        job.created.elapsed().as_secs(),
                    ),
                    Choice::Task(id),
                )
            })
            .collect();
        self.agent_picker(text::AGENT_TASKS_TITLE, choices);
    }
    /// Stops one agent task. The session keeps its access.
    pub(in crate::app) fn stop_agent_task(&mut self, id: &str) {
        if !self
            .agent
            .jobs
            .get(id)
            .is_some_and(|job| !finished(job.state))
        {
            self.status = Some(text::AGENT_TASK_GONE.into());
            return;
        }
        self.terminate_agent_job(id, "cancelled", text::AGENT_STOP_REASON);
        self.status = Some(text::agent_stopped(1));
    }
    fn agent_picker(&mut self, title: &str, choices: Vec<(String, Choice)>) {
        if choices.is_empty() {
            self.status = Some(format!("{title}: no pending items."));
            return;
        }
        let (items, choices) = choices.into_iter().unzip();
        self.picker = Some(Picker {
            kind: PickerKind::Agent(choices),
            title: title.into(),
            items,
            cursor: 0,
            loading: false,
        });
    }
    pub(in crate::app) fn accept_agent_choice(&mut self, choice: Choice) {
        match choice {
            Choice::Session(id) => self.open_agent_access(&id),
            Choice::Proposal(id) => self.open_agent_review(&id),
            Choice::Remembered(client) => {
                let Some(level) = self.agent.remembered.get(&client) else {
                    return;
                };
                self.show_dialog(
                    text::AGENT_REMEMBERED_TITLE,
                    text::agent_remembered_body(&printable(&client), level.choice()),
                    vec![
                        (text::AGENT_KEEP, DialogAction::Dismiss),
                        (text::AGENT_FORGET, DialogAction::AgentForget(client)),
                    ],
                );
            }
            Choice::Task(id) => {
                let Some(job) = self.agent.jobs.get(&id).filter(|job| !finished(job.state)) else {
                    self.status = Some(text::AGENT_TASK_GONE.into());
                    return;
                };
                let body = text::agent_task_body(
                    &self.agent.client(&job.session),
                    jobs::task_kind(&job.output),
                    short(&id),
                    job.state,
                    job.created.elapsed().as_secs(),
                );
                // Keep has the initial focus, so Enter never stops a task by accident.
                self.show_dialog(
                    text::AGENT_TASKS_TITLE,
                    body,
                    vec![
                        (text::AGENT_TASK_KEEP, DialogAction::Dismiss),
                        (text::AGENT_STOP, DialogAction::AgentStopTask(id)),
                    ],
                );
            }
        }
    }

    /// Returns the card of the most urgent agent item that the user did not put off.
    pub(in crate::app) fn agent_card(&self) -> Option<(Notice, crate::ui::agent::Card)> {
        use crate::input::Action;
        let (notice, position, total) = self.agent.notice()?;
        let card = match &notice {
            Notice::Access(session) => crate::ui::agent::Card {
                title: text::AGENT_ACCESS_TITLE,
                message: text::agent_card(&self.agent.client(session), position, total),
                shortcut: self.agent_shortcut(Action::AgentSessions, "sessions", "Agent sessions"),
                primary: text::AGENT_REVIEW,
                secondary: text::AGENT_LATER,
            },
            Notice::Proposal(id) => {
                let proposal = &self.agent.proposals[id];
                crate::ui::agent::Card {
                    title: text::AGENT_PROPOSAL_TITLE,
                    message: text::agent_proposal_card(
                        &self.agent.client(&proposal.session),
                        &printable(&self.proposal_console_name(proposal.console)),
                        proposal.lines,
                        position,
                        total,
                    ),
                    shortcut: self.agent_shortcut(
                        Action::AgentReview,
                        "review",
                        "Review agent changes",
                    ),
                    primary: text::AGENT_REVIEW,
                    secondary: text::AGENT_LATER,
                }
            }
            Notice::Task(id) => {
                let job = &self.agent.jobs[id];
                crate::ui::agent::Card {
                    title: text::AGENT_TASK_TITLE,
                    message: text::agent_task_card(
                        &self.agent.client(&job.session),
                        jobs::task_kind(&job.output),
                        short(id),
                        job.created.elapsed().as_secs(),
                        total,
                    ),
                    shortcut: None,
                    primary: text::AGENT_STOP,
                    secondary: text::AGENT_HIDE,
                }
            }
        };
        Some((notice, card))
    }

    /// Applies a click on the agent card. A click outside its buttons does nothing.
    pub(in crate::app) fn on_agent_card(&mut self, position: ratatui::layout::Position) {
        let targets = self.layout.agent_card;
        let Some(notice) = self.layout.agent_notice.clone() else {
            return;
        };
        if targets.secondary.contains(position) {
            let (Notice::Access(id) | Notice::Proposal(id) | Notice::Task(id)) = notice;
            self.agent.put_off(&id);
            return;
        }
        if !targets.primary.contains(position) {
            return;
        }
        self.pending_key = None;
        self.command_page = None;
        match notice {
            Notice::Access(session) => self.open_agent_access(&session),
            Notice::Proposal(id) => self.open_agent_review(&id),
            Notice::Task(id) => self.stop_agent_task(&id),
        }
    }

    /// Opens the most urgent item: the oldest access request, then the oldest proposal.
    pub(in crate::app) fn review_agent_next(&mut self) {
        if let Some(id) = self.agent.pending_session_ids().first() {
            self.open_agent_access(&id.clone());
        } else if let Some(id) = self.agent.pending_proposal_ids().first() {
            self.open_agent_review(&id.clone());
        } else {
            self.status = Some(text::AGENT_NOTHING.into());
        }
    }

    /// Opens the access question of a session. The first option never gives access.
    fn open_agent_access(&mut self, id: &str) {
        let Some(session) = self
            .agent
            .sessions
            .get(id)
            .filter(|session| !session.alive.is_cancelled())
        else {
            return;
        };
        let client = printable(&session.client);
        if session.state == "authorized" {
            let body = text::agent_revoke_body(
                &client,
                short(id),
                session.level.choice(),
                self.agent.session_tasks(id),
            );
            // The other levels change the access in both directions. Keep stays first.
            let mut options = vec![(text::AGENT_KEEP, DialogAction::Dismiss)];
            let max = self.agent.settings.max_access;
            for grant in [Grant::Allow, Grant::All, Grant::Full] {
                let level = grant.level().expect("a grant with access");
                if level != session.level && level <= max {
                    options.push((level.choice(), DialogAction::AgentAccess(id.into(), grant)));
                }
            }
            options.push((text::AGENT_REVOKE, DialogAction::AgentRevoke(id.into())));
            self.show_dialog(text::AGENT_ACCESS_OPEN_TITLE, body, options);
            return;
        }
        let age = session.requested.elapsed();
        let age = text::transaction_age(age).unwrap_or_else(|| format!("{}s", age.as_secs()));
        let body = text::agent_access_body(&client, short(id), &age);
        let pending = self.agent.pending_session_ids();
        let position = pending
            .iter()
            .position(|item| item == id)
            .map_or(1, |index| index + 1);
        // Reject stays first. The other options stop at the maximum of the settings.
        let max = self.agent.settings.max_access;
        let options = [Grant::Reject, Grant::Allow, Grant::All, Grant::Full]
            .into_iter()
            .filter(|grant| grant.level().is_none_or(|level| level <= max))
            .map(|grant| {
                let name = grant.level().map_or(text::AGENT_REJECT, Level::choice);
                (name, DialogAction::AgentAccess(id.into(), grant))
            })
            .collect();
        self.show_dialog(
            text::agent_queue_title(
                text::AGENT_ACCESS_TITLE,
                position,
                pending.len().max(position),
            ),
            body,
            options,
        );
    }

    /// Selects the console of a proposal and shows its changes in the editor.
    fn open_agent_review(&mut self, id: &str) {
        let Some(proposal) = self
            .agent
            .proposals
            .get(id)
            .filter(|proposal| proposal.state == "pending")
        else {
            self.status = Some(text::AGENT_PROPOSAL_GONE.into());
            return;
        };
        let (console, session) = (proposal.console, proposal.session.clone());
        let Some(index) = self.console_index(console) else {
            self.status = Some(text::AGENT_PROPOSAL_GONE.into());
            return;
        };
        self.select_console(index);
        self.navigator_focused = false;
        self.consoles[index].focus = crate::console::Focus::Editor;
        let open = self.consoles[index]
            .editor
            .review()
            .map(|r| r.source.clone());
        match open {
            Some(Source::Agent(open)) if open == id => return,
            Some(_) => self.close_review(),
            None => {}
        }
        let Some(hunks) = self
            .agent
            .proposals
            .get_mut(id)
            .and_then(|proposal| proposal.hunks.take())
        else {
            return;
        };
        let label = format!("MCP {}", self.agent.client(&session));
        let mut review = Review::diff(Source::Agent(id.into()), label, hunks);
        review.keys = self.review_keys(text::AGENT_LATER);
        self.consoles[index].editor.start_review(review);
        self.agent.reviewing.insert(id.into());
    }

    /// Closes the review of a proposal. The proposal and its decisions wait for later.
    pub(in crate::app) fn agent_review_later(&mut self, id: &str) {
        let Some(index) = self.review_console(id) else {
            return;
        };
        let review = self.consoles[index].editor.take_review();
        self.agent.reviewing.remove(id);
        if let (Some(review), Some(proposal)) = (review, self.agent.proposals.get_mut(id)) {
            proposal.hunks = Some(review.into_hunks());
        }
    }

    /// Returns the console that shows the review of a proposal.
    fn review_console(&self, id: &str) -> Option<usize> {
        self.consoles.iter().position(|console| {
            console
                .editor
                .review()
                .is_some_and(|review| review.source == Source::Agent(id.into()))
        })
    }

    /// Records a decision on a proposal. After the last change, the proposal ends with
    /// `applied`, `rejected`, or `partial`, and the editor stays.
    pub(in crate::app) fn agent_review_progress(&mut self, id: &str) {
        let Some(index) = self.review_console(id) else {
            return;
        };
        let revision = self.consoles[index].editor.revision();
        let Some(proposal) = self.agent.proposals.get_mut(id) else {
            return;
        };
        // The accepted changes give a new revision, so the proposal keeps its place.
        proposal.revision = revision;
        let editor = &mut self.consoles[index].editor;
        if !editor.review().is_some_and(|review| review.finished()) {
            return;
        }
        let outcome = editor
            .take_review()
            .map(|review| review.outcome())
            .unwrap_or_default();
        self.agent.reviewing.remove(id);
        let (session, console) = (proposal.session.clone(), proposal.console);
        proposal.state = match (outcome.accepted, outcome.rejected) {
            (0, _) => "rejected",
            (_, 0) => "applied",
            _ => "partial",
        };
        proposal.decided = Some(outcome);
        proposal.edits.clear();
        proposal._hold = None;
        let state = proposal.state;
        let connection = self.console_connection(console);
        let name = printable(&self.proposal_console_name(console));
        let notice = match state {
            "rejected" => text::agent_rejected(&name),
            "applied" => text::agent_applied(&name),
            _ => text::agent_partial(&name, outcome.accepted, outcome.rejected),
        };
        self.agent_notice(&session, connection, notice);
        if outcome.accepted > 0 && self.agent.owns(console) {
            self.agent_sql_placed(console);
        }
    }

    /// Returns the name of a console, or a mark for a closed console.
    fn proposal_console_name(&self, console: ConsoleId) -> String {
        self.consoles
            .iter()
            .find(|item| item.id == console)
            .map_or_else(|| "Closed console".into(), |item| item.name.clone())
    }

    pub(in crate::app) fn agent_access_decision(&mut self, id: &str, grant: Grant) {
        if let Some(session) = self
            .agent
            .sessions
            .get_mut(id)
            .filter(|session| !session.alive.is_cancelled())
        {
            self.agent.later.remove(id);
            // A decision never goes above the maximum, also from an old dialog.
            let level = grant
                .level()
                .map(|level| level.min(self.agent.settings.max_access));
            session.state = if level.is_some() {
                "authorized"
            } else {
                "denied"
            };
            session.level = level.unwrap_or(Level::Propose);
            let choice = level.map_or(text::AGENT_REJECT, Level::choice);
            if let Some(level) = level
                && self.agent.settings.remember_clients
            {
                // A reconnection of this client uses the same level until SQLN exits.
                self.agent.remembered.insert(session.client.clone(), level);
            }
            let notice = text::agent_access_done(&self.agent.client(id), choice);
            self.ensure_agent_tabs();
            self.agent_notice(id, None, notice);
            self.refresh_tree_consoles();
        }
        if let Some(next) = self.agent.pending_session_ids().first() {
            self.open_agent_access(&next.clone());
        }
    }
    pub(in crate::app) fn agent_revoke(&mut self, id: &str) {
        let client = self.agent.client(id);
        if let Some(session) = self.agent.sessions.get(id) {
            self.agent.remembered.remove(&session.client);
        }
        self.end_agent_session(id, "revoked");
        self.agent_notice(id, None, text::agent_revoked(&client));
    }
    /// Removes the remembered access of a client. Its next session asks the user again.
    pub(in crate::app) fn agent_forget(&mut self, client: &str) {
        if self.agent.remembered.remove(client).is_some() {
            self.status = Some(text::agent_forgotten(&printable(client)));
        }
    }

    /// Cancels the tasks of all sessions. The sessions keep their access.
    pub(in crate::app) fn stop_agent_tasks(&mut self) {
        let jobs: Vec<_> = self
            .agent
            .jobs
            .iter()
            .filter(|(_, job)| !finished(job.state))
            .map(|(id, _)| id.clone())
            .collect();
        for job in &jobs {
            self.terminate_agent_job(job, "cancelled", text::AGENT_STOP_REASON);
        }
        self.status = Some(text::agent_stopped(jobs.len()));
    }

    pub(in crate::app) fn toggle_agent_direct(&mut self) {
        let id = self.console().id;
        if self.agent.direct.remove(&id) {
            self.status = Some("Agent direct editing disabled for this console.".into());
            self.refresh_tree_consoles();
        } else {
            self.show_dialog("Allow direct agent edits",format!("Console: {}\n\nAuthorized agents can edit this console without a proposal until it closes or SQLN exits. SQL changes can save automatically. Nothing executes automatically.",printable(&self.console().name)),vec![("Cancel",DialogAction::Dismiss),("Allow",DialogAction::AgentDirect(id))]);
        }
    }
    pub(in crate::app) fn allow_agent_direct(&mut self, console: ConsoleId) {
        if self.console_index(console).is_some() {
            self.agent.direct.insert(console);
            self.status =
                Some("Agent direct editing enabled for this console until SQLN exits.".into());
            self.refresh_tree_consoles();
        }
    }

    pub(in crate::app) fn agent_edit(
        &mut self,
        session: &str,
        console: ConsoleId,
        revision: u64,
        edits: &[Edit],
        mode: EditMode,
    ) -> Reply {
        let index = self.console_index(console).ok_or_else(missing)?;
        if self.consoles[index].hidden {
            return Err(missing());
        }
        if self.consoles[index].editor.revision() != revision {
            return Err(revision_conflict());
        }
        let edits: Vec<_> = edits
            .iter()
            .cloned()
            .map(|mut edit| {
                edit.text = edit.text.replace("\r\n", "\n").replace('\r', "\n");
                edit
            })
            .collect();
        self.consoles[index].editor.agent_check_edits(&edits)?;
        if mode == EditMode::Direct {
            let all = self
                .agent
                .level(session)
                .is_some_and(|level| level >= Level::Direct);
            if !all && !self.agent.direct.contains(&console) {
                return Err(Error::new(
                    "direct_edit_not_allowed",
                    "Ask the user to enable Agent direct editing for this console, or submit a proposal.",
                ));
            }
            self.consoles[index]
                .editor
                .apply_agent_edits(revision, &edits)?;
            if !self.agent.owns(console) {
                let undo = self.shown_key(crate::input::Action::EditorUndo);
                let notice = text::agent_edited(
                    &self.agent.client(session),
                    &printable(&self.consoles[index].name),
                    undo.as_deref(),
                );
                let connection = self.consoles[index].connection;
                self.agent_notice(session, connection, notice);
            }
            return Ok(
                json!({"status":"applied","revision":self.consoles[index].editor.revision()}),
            );
        }
        if self.agent.proposals.values().any(|proposal| {
            proposal.console == console && matches!(proposal.state, "pending" | "building")
        }) {
            return Err(Error::new(
                "proposal_pending",
                "This console already has an edit proposal. Wait for the user to review it.",
            ));
        }
        if self.agent.proposals.len() >= 64 {
            if let Some(old) = self
                .agent
                .proposals
                .iter()
                .find(|(_, proposal)| !matches!(proposal.state, "pending" | "building"))
                .map(|(id, _)| id.clone())
            {
                self.agent.proposals.remove(&old);
            } else {
                return Err(Error::new(
                    "too_many_proposals",
                    "Review pending edit proposals in SQLN before sending more.",
                ));
            }
        }
        let before = self.consoles[index].editor.text();
        if before.len() > agent::MAX_FRAME {
            return Err(Error::new(
                "request_too_large",
                "This console is too large for an edit proposal. Use a smaller console.",
            ));
        }
        let scratch = agent::Hold::new(before.len().saturating_mul(6), &self.budget)?;
        let before = Text::new(before, &self.budget)?;
        let hold = agent::Hold::new(
            edits
                .iter()
                .map(|edit| edit.text.capacity())
                .sum::<usize>()
                .saturating_mul(2),
            &self.budget,
        )?;
        let budget = self.budget.clone();
        let proposal = uuid::Uuid::new_v4().to_string();
        let sender = self.agent.sender.clone();
        let key = proposal.clone();
        let changes = edits.clone();
        self.agent.proposals.insert(
            proposal.clone(),
            Proposal {
                session: session.into(),
                console,
                revision,
                edits,
                state: "building",
                hunks: None,
                decided: None,
                lines: (0, 0),
                created: std::time::Instant::now(),
                _hold: Some(hold),
            },
        );
        if !self.ensure_manager() {
            self.agent.proposals.remove(&proposal);
            return Err(Error::new(
                "execution_unavailable",
                "Cannot start the edit review worker.",
            ));
        }
        self.manager
            .as_ref()
            .expect("manager")
            .spawn_blocking(move || {
                let _scratch = scratch;
                let after = edited(&before.value, &changes);
                let hunks = diff_hunks(
                    &before.value,
                    &after,
                    0,
                    std::time::Duration::from_millis(250),
                );
                let bytes = hunks.iter().map(|hunk| hunk.bytes()).sum();
                let result = agent::Hold::new(bytes, &budget).map(|hold| (hunks, hold));
                let _ = sender.send(Work::Diff(key, result));
            });
        Ok(json!({"job_id":proposal,"kind":"proposal","status":"building"}))
    }
}

fn edited(before: &str, edits: &[Edit]) -> String {
    let lines: Vec<_> = before.split('\n').collect();
    let offset = |position: agent::Position| {
        lines
            .iter()
            .take(position.line)
            .map(|line| line.len() + 1)
            .sum::<usize>()
            + lines[position.line]
                .chars()
                .take(position.column)
                .map(char::len_utf8)
                .sum::<usize>()
    };
    let mut output = String::new();
    let mut previous = 0;
    for edit in edits {
        let (start, end) = (offset(edit.start), offset(edit.end));
        output.push_str(&before[previous..start]);
        output.push_str(&edit.text);
        previous = end;
    }
    output.push_str(&before[previous..]);
    output
}
