//! Immutable completion targets and the command row of the suggestion list.
use super::{ListAction, SqlEditor, SuggestionList};
use crate::ai::Context;
use crate::catalog::Catalog;
use crate::complete::Completion;
use edtui::Index2;

#[derive(Clone)]
pub struct Target {
    pub revision: u64,
    pub cursor: Index2,
    pub start: Index2,
    pub end: Index2,
    /// The text of the first line before `start`, and of the last line after `end`.
    pub head: String,
    pub tail: String,
    /// The number of lines at the time of the request. An edit that changes the
    /// line count moves the requested line, and the anchor follows it.
    pub line_count: usize,
    pub context: Context,
}
impl SqlEditor {
    pub fn set_list_area(&mut self, area: (ratatui::layout::Rect, usize)) {
        self.list_area = area;
    }
    pub fn click_suggestion(&mut self, column: u16, row: u16) -> bool {
        let Some(list) = &mut self.list else {
            return false;
        };
        if !self.list_area.0.contains((column, row).into()) {
            return false;
        }
        let index = usize::from(row - self.list_area.0.y) + self.list_area.1;
        if list.ai && index == 0 {
            self.ai_requested = true;
            self.close_suggestions();
        } else {
            list.selected = index - usize::from(list.ai);
            self.accept_suggestion();
        }
        true
    }
    pub fn set_ai_available(&mut self, available: bool) {
        self.ai_available = available;
        let ai_row = self.ai_row();
        if let Some(list) = &mut self.list {
            list.ai = ai_row;
            if !available && list.selected == usize::MAX {
                self.close_suggestions();
            }
        }
    }
    pub fn take_ai_request(&mut self) -> bool {
        std::mem::take(&mut self.ai_requested)
    }
    /// Sets the anchors of the running completions. Edits on other lines move them,
    /// and the application clears them when the answers show or the requests end.
    pub fn set_ai_pending(&mut self, at: Vec<(Index2, std::time::Instant)>) {
        self.ai_pending = at;
    }
    /// Returns the anchors of the running completions with their start times.
    pub fn ai_pending(&self) -> &[(Index2, std::time::Instant)] {
        &self.ai_pending
    }
    pub fn open_explicit_suggestions(&mut self, catalog: Option<&Catalog>) {
        if self.is_inserting() {
            self.update_list(ListAction::Open, catalog);
        } else {
            self.close_suggestions();
        }
        if self.ai_row() && self.list.is_none() {
            self.list = Some(SuggestionList {
                completion: Completion::default(),
                selected: usize::MAX,
                ai: true,
            });
        } else if self.ai_available && self.selection_range().is_some() && self.list.is_none() {
            // The AI completion works at the cursor only. Report why no AI row shows.
            self.ai_refused = true;
        }
    }
    /// Returns whether the AI row can show: AI enabled and no selection, because the
    /// completion works at the cursor only.
    pub(crate) fn ai_row(&self) -> bool {
        self.ai_available && self.selection_range().is_none()
    }
    pub fn take_ai_refused(&mut self) -> bool {
        std::mem::take(&mut self.ai_refused)
    }
    pub fn ai_target(&mut self, schema: Option<String>) -> Target {
        let revision = self.revision;
        let cursor = self.state.cursor;
        let (start, end) = (cursor, cursor);
        let dialect = self.dialect;
        let line_count = self.state.lines.len();
        let head: String = buffer_row(self, start.row).iter().take(start.col).collect();
        let tail: String = buffer_row(self, end.row).iter().skip(end.col).collect();
        let snapshot = self.snapshot();
        let (begin, finish) = (snapshot.offset(start), snapshot.offset(end));
        let statements = crate::sql::split(&snapshot.text, dialect);
        let first = statements
            .iter()
            .find(|s| s.start <= begin && s.end >= begin)
            .map_or(begin, |s| s.start);
        let last = statements
            .iter()
            .find(|s| s.start <= finish && s.end >= finish)
            .map_or(finish, |s| s.end);
        let mut left = first.saturating_sub(4096);
        while !snapshot.text.is_char_boundary(left) {
            left += 1;
        }
        let mut right = (last + 4096).min(snapshot.text.len());
        while !snapshot.text.is_char_boundary(right) {
            right -= 1;
        }
        Target {
            revision,
            cursor,
            start,
            end,
            head,
            tail,
            line_count,
            context: Context {
                prefix: snapshot.text[left..begin].into(),
                original: snapshot.text[begin..finish].into(),
                suffix: snapshot.text[finish..right].into(),
                dialect,
                schema,
            },
        }
    }
    /// Returns the current row of the requested line, or None when an edit changed
    /// that line. Edits on other lines move the row with the text.
    pub fn ai_line_row(&self, target: &Target) -> Option<usize> {
        let line = format!("{}{}{}", target.head, target.context.original, target.tail);
        super::review::anchored_row(
            &self.state.lines,
            target.start.row,
            &line,
            target.line_count,
        )
    }
    /// Inserts ghost text at its anchor as one undo step. The cursor and the selection
    /// of the user stay in place.
    pub fn apply_ghost(&mut self, at: Index2, text: &str) -> Result<(), String> {
        let revision = self.revision;
        let edit = crate::agent::Edit {
            start: crate::agent::Position {
                line: at.row,
                column: at.col,
            },
            end: crate::agent::Position {
                line: at.row,
                column: at.col,
            },
            text: text.to_owned(),
        };
        self.apply_agent_edits(revision, &[edit])
            .map_err(|_| "The editor changed. Request another completion.".to_owned())
    }
    /// Ends the selection and the visual mode.
    pub fn clear_selection(&mut self) {
        self.leave_visual_mode();
        self.anchor = None;
        self.state.selection = None;
        self.sync_selection();
    }
}

fn buffer_row(editor: &SqlEditor, row: usize) -> &[char] {
    super::buffer::row(&editor.state.lines, row)
}
