//! User-only authorization, revocation, and acceptance of edit proposals.

use super::*;
use crate::agent::{Edit, EditMode};
use crate::grid::printable;
use crate::ui::wrap::WrappedText;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use serde_json::json;

impl App {
    pub(in crate::app) fn open_agent_sessions(&mut self) {
        let mut choices: Vec<_> = self
            .agent
            .sessions
            .iter()
            .filter(|(_, session)| !session.alive.is_cancelled())
            .map(|(id, session)| {
                (
                    format!(
                        "{} [{}] — {}",
                        printable(&session.client),
                        &id[..8.min(id.len())],
                        session.state
                    ),
                    Choice::Session(id.clone()),
                )
            })
            .collect();
        choices.sort_by(|a, b| a.0.cmp(&b.0));
        self.agent_picker("Agent sessions", choices);
    }
    pub(in crate::app) fn open_agent_proposals(&mut self) {
        let choices = self
            .agent
            .proposals
            .iter()
            .filter(|(_, proposal)| proposal.state == "pending")
            .map(|(id, proposal)| {
                let name = self
                    .consoles
                    .iter()
                    .find(|console| console.id == proposal.console)
                    .map_or("Closed console", |console| console.name.as_str());
                (
                    format!("{} — revision {}", printable(name), proposal.revision),
                    Choice::Proposal(id.clone()),
                )
            })
            .collect();
        self.agent_picker("Review agent changes", choices);
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
            Choice::Session(id) => {
                let Some(session) = self
                    .agent
                    .sessions
                    .get(&id)
                    .filter(|session| !session.alive.is_cancelled())
                else {
                    return;
                };
                let authorized = session.state == "authorized";
                let body = format!(
                    "Client: {}\nSession: {}\n\nAccess permits reading consoles, loaded data, and catalog metadata. It permits read-only queries and edit proposals.\n\nThe client name is reported, not verified.",
                    printable(&session.client),
                    id
                );
                self.show_dialog(
                    if authorized {
                        "Revoke agent access"
                    } else {
                        "Allow agent access"
                    },
                    body,
                    if authorized {
                        vec![
                            ("Keep access", DialogAction::Dismiss),
                            ("Revoke", DialogAction::AgentRevoke(id)),
                        ]
                    } else {
                        vec![
                            ("Reject", DialogAction::AgentAccess(id.clone(), false)),
                            ("Allow", DialogAction::AgentAccess(id, true)),
                        ]
                    },
                );
            }
            Choice::Proposal(id) => {
                let Some(proposal) = self
                    .agent
                    .proposals
                    .get(&id)
                    .filter(|proposal| proposal.state == "pending")
                else {
                    return;
                };
                let Some(diff) = proposal.diff.as_ref() else {
                    return;
                };
                let title = format!(
                    "Agent edit — console {} — revision {}",
                    proposal.console, proposal.revision
                );
                self.agent.review = Some(crate::ui::agent::Review {
                    proposal: id,
                    title,
                    contents: WrappedText::new(diff.value.clone()),
                    scroll: 0,
                    accept: false,
                    buttons: Default::default(),
                    _hold: match agent::Hold::new(diff.value.capacity(), &self.budget) {
                        Ok(hold) => hold,
                        Err(error) => {
                            self.status = Some(error.message);
                            return;
                        }
                    },
                });
            }
        }
    }

    pub(in crate::app) fn agent_access_decision(&mut self, id: &str, allow: bool) {
        if let Some(session) = self
            .agent
            .sessions
            .get_mut(id)
            .filter(|session| !session.alive.is_cancelled())
        {
            session.state = if allow { "authorized" } else { "denied" };
            self.status = Some(
                if allow {
                    "Agent access allowed."
                } else {
                    "Agent access rejected."
                }
                .into(),
            );
        }
    }
    pub(in crate::app) fn agent_revoke(&mut self, id: &str) {
        self.end_agent_session(id, "revoked");
        self.status = Some("Agent access revoked. Its active queries are cancelling.".into());
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
            if !self.agent.direct.contains(&console) {
                return Err(Error::new(
                    "direct_edit_not_allowed",
                    "Ask the user to enable Agent direct editing for this console, or submit a proposal.",
                ));
            }
            self.consoles[index]
                .editor
                .apply_agent_edits(revision, &edits)?;
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
                diff: None,
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
                let diff = similar::TextDiff::configure()
                    .timeout(std::time::Duration::from_millis(250))
                    .diff_lines(&before.value, &after)
                    .unified_diff()
                    .header("Current SQL", "Proposed SQL")
                    .to_string();
                let _ = sender.send(Work::Diff(key, Text::new(diff, &budget)));
            });
        Ok(json!({"proposal_id":proposal,"status":"building"}))
    }

    pub(in crate::app) fn on_agent_review_key(&mut self, key: KeyEvent) {
        let Some(review) = &mut self.agent.review else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.agent.review = None;
            }
            KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('h' | 'l') => review.accept = !review.accept,
            KeyCode::Up | KeyCode::Char('k') => review.scroll = review.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => review.scroll = review.scroll.saturating_add(1),
            KeyCode::PageUp => review.scroll = review.scroll.saturating_sub(10),
            KeyCode::PageDown => review.scroll = review.scroll.saturating_add(10),
            KeyCode::Enter => {
                let id = review.proposal.clone();
                let accept = review.accept;
                self.agent.review = None;
                self.decide_agent_proposal(&id, accept);
            }
            _ => {}
        }
    }
    pub(in crate::app) fn on_agent_review_mouse(&mut self, event: MouseEvent) {
        let Some(review) = &mut self.agent.review else {
            return;
        };
        match event.kind {
            MouseEventKind::ScrollUp => review.scroll = review.scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => review.scroll = review.scroll.saturating_add(3),
            MouseEventKind::Down(MouseButton::Left) => {
                let point = ratatui::layout::Position::new(event.column, event.row);
                if let Some(index) = review
                    .buttons
                    .iter()
                    .position(|button| button.contains(point))
                {
                    let id = review.proposal.clone();
                    self.agent.review = None;
                    self.decide_agent_proposal(&id, index == 1);
                }
            }
            _ => {}
        }
    }
    fn decide_agent_proposal(&mut self, id: &str, accept: bool) {
        let Some(proposal) = self
            .agent
            .proposals
            .get_mut(id)
            .filter(|proposal| proposal.state == "pending")
        else {
            self.status =
                Some("This agent proposal is no longer available. Request a new proposal.".into());
            return;
        };
        if !accept {
            proposal.state = "rejected";
            proposal.edits.clear();
            proposal._hold = None;
            proposal.diff = None;
            self.status = Some("Agent edit rejected.".into());
            return;
        }
        let (console, revision, edits) =
            (proposal.console, proposal.revision, proposal.edits.clone());
        let result = self
            .console_index(console)
            .ok_or_else(missing)
            .and_then(|index| {
                self.consoles[index]
                    .editor
                    .apply_agent_edits(revision, &edits)
            });
        let proposal = self.agent.proposals.get_mut(id).expect("checked proposal");
        proposal.diff = None;
        proposal.edits.clear();
        proposal._hold = None;
        match result {
            Ok(()) => {
                proposal.state = "applied";
                self.status =
                    Some("Agent edit applied. SQL does not execute automatically.".into());
            }
            Err(error) => {
                proposal.state = "revision_conflict";
                self.status = Some(error.message);
            }
        }
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
