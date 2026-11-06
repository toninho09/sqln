//! Inline reviews in the editor: keys, the mouse, and the decisions of the origin.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::App;
use crate::console::Focus;
use crate::editor::review::{Control, Keys, Source};
use crate::input::{Action, Chord, Lookup, Region};
use crate::ui::text;

const WHEEL_ROWS: isize = 3;
const PAGE_ROWS: isize = 10;
const SIDE_COLUMNS: isize = 4;

impl App {
    /// Returns the keys that the review bar shows.
    pub(super) fn review_keys(&self, close_label: &'static str) -> Keys {
        let key = |action| self.shown_key(action).unwrap_or_default();
        Keys {
            accept: key(Action::ReviewAccept),
            reject: key(Action::ReviewReject),
            accept_all: key(Action::ReviewAcceptAll),
            reject_all: key(Action::ReviewRejectAll),
            close: key(Action::ReviewClose),
            close_label,
        }
    }

    /// Returns the keys of a ghost text: Tab or another key of `reviewAccept` that is
    /// not a character, and the close key.
    pub(super) fn ghost_keys(&self) -> Keys {
        let accept = self
            .keymap
            .keys(Action::ReviewAccept)
            .iter()
            .find(|key| Chord::parse(key).is_ok_and(|chord| !chord.is_plain_char()))
            .map(|key| text::key_label(key))
            .unwrap_or_default();
        Keys {
            accept,
            close: self.shown_key(Action::ReviewClose).unwrap_or_default(),
            close_label: "Reject",
            ..Keys::default()
        }
    }

    /// Returns true when the active editor shows a review or a ghost answer and has
    /// the focus.
    pub(super) fn review_active(&self) -> bool {
        self.editor_has_focus()
            && (self.console().editor.review().is_some() || self.console().editor.has_ghosts())
    }

    /// Returns true when an action would change or run the text of an inline diff.
    /// A ghost blocks nothing: the editor stays editable around it.
    pub(super) fn review_blocks(&self, action: Action) -> bool {
        self.review_active()
            && self.console().editor.review().is_some()
            && !matches!(
                action,
                Action::EditorAiAccept | Action::EditorAiReject | Action::EditorAiCancel
            )
            && (action.region() == Region::Editor
                || matches!(
                    action,
                    Action::FormatSql
                        | Action::ToggleComment
                        | Action::Run
                        | Action::RunScript
                        | Action::Explain
                        | Action::ExplainAnalyze
                ))
    }

    pub(super) fn review_command_available(&self, action: Action) -> bool {
        if !self.editor_has_focus() {
            return false;
        }
        if self.console().editor.has_ghosts() {
            return matches!(
                action,
                Action::ReviewAccept | Action::ReviewReject | Action::ReviewClose
            );
        }
        self.console().editor.review().is_some()
    }

    /// Applies a key during a review. A ghost keeps its place: other keys go to the
    /// editor. A diff refuses keys that change the text, and gives the others on.
    pub(super) fn on_review_key(&mut self, key: KeyEvent) -> bool {
        if !self.review_active() {
            return false;
        }
        let chord = Chord::from_event(key);
        let plain = (key.modifiers - KeyModifiers::SHIFT).is_empty();
        let text_key = plain && matches!(key.code, KeyCode::Char(_));
        let action = match self.keymap.lookup(Region::Review, chord) {
            Lookup::Action(action) => Some(action),
            _ => None,
        };
        if self.console().editor.review().is_none() && self.console().editor.has_ghosts() {
            // A ghost stays until the user decides. Every other key goes to the
            // editor, so the editor keeps working around the ghosts.
            match action {
                _ if text_key => return false,
                Some(Action::ReviewAccept | Action::ReviewReject) => {
                    self.decide_ghost(action == Some(Action::ReviewAccept));
                }
                Some(Action::ReviewClose) => self.escape_ghost(),
                _ => return false,
            }
            return true;
        }
        if let Some(action) = action {
            self.perform_review(action);
            return true;
        }
        let scroll = match key.code {
            KeyCode::PageUp => Some((-PAGE_ROWS, 0)),
            KeyCode::PageDown => Some((PAGE_ROWS, 0)),
            KeyCode::Left => Some((0, -SIDE_COLUMNS)),
            KeyCode::Right => Some((0, SIDE_COLUMNS)),
            _ => None,
        };
        if let Some((rows, columns)) = scroll.filter(|_| plain) {
            self.console_mut().editor.review_scroll(rows, columns);
            return true;
        }
        let global = !text_key
            && !matches!(self.keymap.lookup(Region::Editor, chord), Lookup::Action(_))
            && self.keymap.lookup(Region::Global, chord) != Lookup::None;
        if global {
            return false;
        }
        self.refuse_review_edit();
        true
    }

    /// Explains why the editor does not take the key.
    pub(super) fn refuse_review_edit(&mut self) {
        let keys = self.console().editor.review().map(|r| r.keys.clone());
        if let Some(keys) = keys {
            self.status = Some(text::review_read_only(
                &keys.accept,
                &keys.reject,
                &keys.close,
                keys.close_label,
            ));
        }
    }

    pub(super) fn perform_review(&mut self, action: Action) {
        if !self.review_command_available(action) {
            return;
        }
        if self.console().editor.review().is_none() && self.console().editor.has_ghosts() {
            match action {
                Action::ReviewAccept => self.decide_ghost(true),
                Action::ReviewReject => self.decide_ghost(false),
                Action::ReviewClose => self.escape_ghost(),
                _ => {}
            }
            return;
        }
        match action {
            Action::ReviewAccept => self.decide_review(true, false),
            Action::ReviewReject => self.decide_review(false, false),
            Action::ReviewAcceptAll => self.decide_review(true, true),
            Action::ReviewRejectAll => self.decide_review(false, true),
            Action::ReviewNext => self.console_mut().editor.review_step(true),
            Action::ReviewPrevious => self.console_mut().editor.review_step(false),
            Action::ReviewClose => self.close_review(),
            _ => {}
        }
    }

    /// Decides the current change, or all changes, and reports to the origin.
    fn decide_review(&mut self, accept: bool, all: bool) {
        let result = self.console_mut().editor.review_decide(accept, all);
        if let Err(error) = result {
            self.status = Some(error.message);
            self.close_review();
            return;
        }
        let Some(source) = self.console().editor.review().map(|r| r.source.clone()) else {
            return;
        };
        match source {
            // The AI completion opens ghosts only; a diff review comes from an agent.
            Source::Ai => {}
            Source::Agent(id) => self.agent_review_progress(&id),
        }
    }

    /// Closes the review of the active console: a rejection for AI, and **Later** for
    /// an agent proposal.
    pub(super) fn close_review(&mut self) {
        let Some(source) = self.console().editor.review().map(|r| r.source.clone()) else {
            return;
        };
        match source {
            // The AI completion opens ghosts only; a diff review comes from an agent.
            Source::Ai => {
                self.console_mut().editor.take_review();
            }
            Source::Agent(id) => self.agent_review_later(&id),
        }
    }

    /// Applies a mouse event in the editor during a review. It returns false when the
    /// event goes on to the editor.
    pub(super) fn on_review_mouse(&mut self, mouse: MouseEvent) -> bool {
        let position = Position::new(mouse.column, mouse.row);
        if self.drag.is_some()
            || !self.console_on_screen()
            || self.home()
            || !self.review_active()
            || !self.layout.console.editor.contains(position)
        {
            return false;
        }
        let ghost = self.console().editor.review().is_none() && self.console().editor.has_ghosts();
        let target = if ghost {
            self.console()
                .editor
                .ghost_targets()
                .iter()
                .rev()
                .find(|(_, area)| area.contains(position))
                .map(|(control, _)| *control)
        } else {
            self.console().editor.review().and_then(|review| {
                review
                    .targets
                    .iter()
                    .rev()
                    .find(|(_, area)| area.contains(position))
                    .map(|(control, _)| *control)
            })
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.navigator_focused = false;
                self.console_mut().focus = Focus::Editor;
                let action = match target {
                    Some(Control::AcceptAll) => Some(Action::ReviewAcceptAll),
                    Some(Control::RejectAll) => Some(Action::ReviewRejectAll),
                    Some(Control::Close) => Some(Action::ReviewClose),
                    Some(Control::Decide { hunk, accept }) => {
                        // The buttons of one change decide that change.
                        self.console_mut().editor.review_select(hunk);
                        Some(if accept {
                            Action::ReviewAccept
                        } else {
                            Action::ReviewReject
                        })
                    }
                    Some(Control::GhostDecide { ghost, accept }) => {
                        if let Err(error) = self.console_mut().editor.ghost_decide(ghost, accept) {
                            self.status = Some(error);
                        }
                        None
                    }
                    Some(Control::Hunk(hunk)) if !ghost => {
                        self.console_mut().editor.review_select(hunk);
                        None
                    }
                    _ => None,
                };
                match action {
                    Some(action) => self.perform_review(action),
                    // A click moves the cursor of the editor; the ghosts stay.
                    None if ghost => self.console_mut().editor.point_ghost(mouse),
                    None => {}
                }
            }
            // A drag selects text of the editor while ghost text shows.
            MouseEventKind::Drag(MouseButton::Left) if ghost => {
                self.console_mut().editor.point_ghost(mouse);
            }
            MouseEventKind::ScrollUp => {
                self.console_mut().editor.review_scroll(-WHEEL_ROWS, 0);
            }
            MouseEventKind::ScrollDown => {
                self.console_mut().editor.review_scroll(WHEEL_ROWS, 0);
            }
            MouseEventKind::ScrollLeft => {
                self.console_mut().editor.review_scroll(0, -SIDE_COLUMNS);
            }
            MouseEventKind::ScrollRight => {
                self.console_mut().editor.review_scroll(0, SIDE_COLUMNS);
            }
            MouseEventKind::Up(_) | MouseEventKind::Moved => return false,
            _ => {}
        }
        true
    }

    /// Returns true when a paste goes no further: a diff review is read only. A ghost
    /// stays, and the paste goes to the editor: the anchor rule keeps or closes it.
    pub(super) fn review_paste(&mut self) -> bool {
        if !self.review_active() || self.console().editor.review().is_none() {
            return false;
        }
        self.refuse_review_edit();
        true
    }
}
