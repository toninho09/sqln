//! IDE keys over EdTUI. EdTUI stays in insert mode, and this layer owns the selection.
//! The selection range excludes the character under the cursor, as in an IDE.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::actions::{DeleteChar, Redo, SwitchMode, Undo};
use edtui::{EditorMode, Index2};

use super::buffer::{
    document_end, last_row, next_position, previous_position, row, text_in, word_left, word_right,
};
use super::{KeyboardMode, SqlEditor};

pub(super) const INDENT: &str = "    ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditKind {
    Typing,
    Deleting,
    Other,
}

/// An edit joins the previous undo step when its kind and its position continue that step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct UndoGroup {
    kind: EditKind,
    cursor: Index2,
}

impl SqlEditor {
    pub(super) fn on_conventional_key(&mut self, key: KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Left => self.move_horizontal(false, alt, shift),
            KeyCode::Right => self.move_horizontal(true, alt, shift),
            KeyCode::Up => self.move_vertical(-1, shift),
            KeyCode::Down => self.move_vertical(1, shift),
            KeyCode::PageUp => self.move_vertical(-self.page_rows(), shift),
            KeyCode::PageDown => self.move_vertical(self.page_rows(), shift),
            KeyCode::Home if ctrl => self.move_to(Index2::new(0, 0), shift),
            KeyCode::End if ctrl => self.move_to(document_end(&self.state.lines), shift),
            KeyCode::Home => self.move_to(Index2::new(self.state.cursor.row, 0), shift),
            KeyCode::End => {
                let cursor = self.state.cursor.row;
                self.move_to(
                    Index2::new(cursor, row(&self.state.lines, cursor).len()),
                    shift,
                );
            }
            KeyCode::Backspace => self.delete_backward(alt || ctrl),
            KeyCode::Delete => self.delete_forward(alt || ctrl),
            KeyCode::Enter if !ctrl && !shift => self.type_text("\n"),
            KeyCode::Tab if self.selected_range().is_none() => self.type_text(INDENT),
            KeyCode::Char(c) if ctrl => self.on_control_key(c.to_ascii_lowercase(), shift),
            KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                self.type_text(c.encode_utf8(&mut [0; 4]));
            }
            _ => {}
        }
    }

    fn on_control_key(&mut self, c: char, shift: bool) {
        match c {
            'a' => {
                self.anchor = Some(Index2::new(0, 0));
                self.state.cursor = document_end(&self.state.lines);
                self.break_undo_group();
                self.sync_selection();
            }
            'c' => self.copy_selection(),
            'x' => self.cut_selection(),
            'v' => self.paste_from_clipboard(),
            'z' if shift => self.redo(),
            'z' => self.undo(),
            'y' => self.redo(),
            _ => {}
        }
    }

    fn page_rows(&self) -> isize {
        self.page_height.max(1) as isize
    }

    fn move_to(&mut self, target: Index2, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.state.cursor);
        } else {
            self.anchor = None;
        }
        self.state.cursor = target;
        self.preferred_col = None;
        self.break_undo_group();
        self.sync_selection();
    }

    fn move_horizontal(&mut self, forward: bool, by_word: bool, extend: bool) {
        let lines = &self.state.lines;
        let cursor = self.state.cursor;
        let target = match (self.selected_range(), extend, by_word) {
            (Some((start, _)), false, false) if !forward => start,
            (Some((_, end)), false, false) if forward => end,
            (_, _, true) if forward => word_right(lines, cursor),
            (_, _, true) => word_left(lines, cursor),
            _ if forward => next_position(lines, cursor),
            _ => previous_position(lines, cursor),
        };
        self.move_to(target, extend);
    }

    fn move_vertical(&mut self, rows: isize, extend: bool) {
        let cursor = self.state.cursor;
        let target_row = cursor
            .row
            .saturating_add_signed(rows)
            .min(last_row(&self.state.lines));
        let wanted = self.preferred_col.unwrap_or(cursor.col);
        let col = wanted.min(row(&self.state.lines, target_row).len());
        self.move_to(Index2::new(target_row, col), extend);
        self.preferred_col = Some(wanted);
    }

    /// Writes EdTUI selection fields. EdTUI does not export the `Selection` type,
    /// so `SwitchMode` creates the value. EdTUI includes the end position.
    pub(super) fn sync_selection(&mut self) {
        if self.keyboard() == KeyboardMode::Vim {
            return;
        }
        let Some((start, end)) = self.selected_range() else {
            self.state.selection = None;
            return;
        };
        if self.state.selection.is_none() {
            self.state.execute(SwitchMode(EditorMode::Visual));
            self.state.mode = EditorMode::Insert;
        }
        let last_selected = previous_position(&self.state.lines, end);
        if let Some(selection) = &mut self.state.selection {
            selection.start = start;
            selection.end = last_selected;
        }
    }

    pub(super) fn selected_range(&self) -> Option<(Index2, Index2)> {
        let anchor = self.anchor?;
        let cursor = self.state.cursor;
        if anchor == cursor {
            None
        } else if anchor < cursor {
            Some((anchor, cursor))
        } else {
            Some((cursor, anchor))
        }
    }

    pub(super) fn selected_text_conventional(&self) -> Option<String> {
        let (start, end) = self.selected_range()?;
        Some(text_in(&self.state.lines, start, end))
    }

    /// Inserts text over the selection. The removed text stays out of the clipboard.
    pub(super) fn type_text(&mut self, text: &str) {
        let (start, end, kind) = match self.selected_range() {
            Some((start, end)) => (start, end, EditKind::Other),
            None => (self.state.cursor, self.state.cursor, EditKind::Typing),
        };
        if text.trim().is_empty() {
            self.break_undo_group();
        }
        self.edit(start, end, text, kind);
    }

    fn delete_backward(&mut self, by_word: bool) {
        if let Some((start, end)) = self.selected_range() {
            self.edit(start, end, "", EditKind::Other);
            return;
        }
        let cursor = self.state.cursor;
        let lines = &self.state.lines;
        let start = if by_word {
            word_left(lines, cursor)
        } else {
            previous_position(lines, cursor)
        };
        self.edit(start, cursor, "", EditKind::Deleting);
    }

    fn delete_forward(&mut self, by_word: bool) {
        if let Some((start, end)) = self.selected_range() {
            self.edit(start, end, "", EditKind::Other);
            return;
        }
        let cursor = self.state.cursor;
        let lines = &self.state.lines;
        let end = if by_word {
            word_right(lines, cursor)
        } else {
            next_position(lines, cursor)
        };
        self.edit(cursor, end, "", EditKind::Deleting);
    }

    pub(super) fn break_undo_group(&mut self) {
        self.undo_group = None;
    }

    pub(super) fn edit(&mut self, start: Index2, end: Index2, text: &str, kind: EditKind) {
        if start == end && text.is_empty() {
            return;
        }
        let continues = self.undo_group
            == Some(UndoGroup {
                kind,
                cursor: self.state.cursor,
            })
            && kind != EditKind::Other;
        if !continues {
            // `DeleteChar(0)` is the public way to record an undo point. It changes no text.
            self.state.execute(DeleteChar(0));
        }
        let cursor = super::buffer::replace_range(&mut self.state.lines, start, end, text);
        self.state.cursor = cursor;
        self.anchor = None;
        self.preferred_col = None;
        self.undo_group = Some(UndoGroup { kind, cursor });
        self.redo_depth = 0;
        self.sync_selection();
        self.mark_changed();
    }

    fn undo(&mut self) {
        let before = self.state.lines.clone();
        self.state.execute(Undo);
        self.after_history_step();
        if self.state.lines != before {
            self.redo_depth += 1;
            self.mark_changed();
        }
    }

    /// EdTUI keeps old redo entries after a new edit, so a depth counter hides them.
    fn redo(&mut self) {
        if self.redo_depth == 0 {
            return;
        }
        self.redo_depth -= 1;
        self.state.execute(Redo);
        self.after_history_step();
        self.mark_changed();
    }

    fn after_history_step(&mut self) {
        self.anchor = None;
        self.preferred_col = None;
        self.state.mode = EditorMode::Insert;
        self.break_undo_group();
        self.sync_selection();
    }
}
