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
    pub selection: Option<(Index2, Index2)>,
    pub start: Index2,
    pub end: Index2,
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
        if let Some(list) = &mut self.list {
            list.ai = available;
            if !available && list.selected == usize::MAX {
                self.close_suggestions();
            }
        }
    }
    pub fn take_ai_request(&mut self) -> bool {
        std::mem::take(&mut self.ai_requested)
    }
    pub fn open_explicit_suggestions(&mut self, catalog: Option<&Catalog>) {
        if self.is_inserting() && (self.selection_range().is_none() || !self.ai_available) {
            self.update_list(ListAction::Open, catalog);
        } else {
            self.close_suggestions();
        }
        if self.ai_available && self.list.is_none() {
            self.list = Some(SuggestionList {
                completion: Completion::default(),
                selected: usize::MAX,
                ai: true,
            });
        }
    }
    pub fn ai_target(&mut self, schema: Option<String>) -> Target {
        let revision = self.revision;
        let cursor = self.state.cursor;
        let selection = self.selection_range();
        let (start, end) = selection.unwrap_or((cursor, cursor));
        let dialect = self.dialect;
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
            selection,
            start,
            end,
            context: Context {
                prefix: snapshot.text[left..begin].into(),
                original: snapshot.text[begin..finish].into(),
                suffix: snapshot.text[finish..right].into(),
                dialect,
                schema,
            },
        }
    }
    pub fn ai_target_current(&self, target: &Target) -> bool {
        self.revision == target.revision
            && self.state.cursor == target.cursor
            && self.selection_range() == target.selection
    }
    pub fn apply_ai(&mut self, target: &Target, text: String) -> Result<(), String> {
        if !self.ai_target_current(target) {
            return Err("The editor changed. Request another completion.".into());
        }
        let edit = crate::agent::Edit {
            start: crate::agent::Position {
                line: target.start.row,
                column: target.start.col,
            },
            end: crate::agent::Position {
                line: target.end.row,
                column: target.end.col,
            },
            text,
        };
        self.apply_agent_edits(target.revision, &[edit])
            .map_err(|_| "The editor changed. Request another completion.".to_owned())?;
        self.leave_visual_mode();
        self.anchor = None;
        self.state.selection = None;
        self.sync_selection();
        Ok(())
    }
}
