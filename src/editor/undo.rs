//! The undo history of the conventional mode. EdTUI keeps a copy of the text for each
//! step, so a step here keeps only the text that changed.
//!
//! The Vim mode keeps the history of EdTUI, because EdTUI records those edits itself.

use std::collections::VecDeque;
use std::mem::size_of;

use edtui::actions::DeleteChar;
use edtui::{EditorState, Index2, Lines};

use super::buffer::{replace_range, text_in};

/// The default limit of the history of one console.
pub const DEFAULT_UNDO_LIMIT: usize = 64 * 1024 * 1024;
/// The stack of EdTUI keeps this number of entries.
const EDTUI_STEPS: usize = 100;

/// A step puts `text` between `start` and `end`, and puts the cursor at `cursor`.
#[derive(Debug)]
struct Step {
    start: Index2,
    end: Index2,
    text: String,
    cursor: Index2,
}

impl Step {
    fn cost(&self) -> usize {
        size_of::<Self>() + self.text.capacity()
    }

    /// Applies the step and returns the step that reverses it.
    fn apply(self, lines: &mut Lines, cursor: &mut Index2) -> Self {
        let text = text_in(lines, self.start, self.end);
        let end = replace_range(lines, self.start, self.end, &self.text);
        let back = Self {
            start: self.start,
            end,
            text,
            cursor: *cursor,
        };
        *cursor = self.cursor;
        back
    }
}

#[derive(Debug)]
pub(super) struct UndoHistory {
    /// The oldest step is at the front.
    undo: VecDeque<Step>,
    /// The next step to apply is at the back.
    redo: VecDeque<Step>,
    bytes: usize,
    limit: usize,
}

impl Default for UndoHistory {
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            bytes: 0,
            limit: DEFAULT_UNDO_LIMIT,
        }
    }
}

impl UndoHistory {
    /// Records an edit. The new text is between `start` and `end`, `text` is the text
    /// that it replaced, and `cursor` is the cursor before the edit.
    pub fn push(&mut self, start: Index2, end: Index2, text: String, cursor: Index2) {
        self.clear_redo();
        let step = Step {
            start,
            end,
            text,
            cursor,
        };
        self.bytes += step.cost();
        self.undo.push_back(step);
        self.trim();
    }

    /// Adds an edit to the last step. The edit replaced `removed` between `start` and
    /// `end`, it touches the text of that step, and its new text ends at `after`.
    pub fn join(&mut self, start: Index2, end: Index2, removed: &str, after: Index2) {
        let Some(step) = self.undo.back_mut() else {
            return;
        };
        self.bytes -= step.cost();
        if start != end && end == step.start {
            step.text.insert_str(0, removed);
            step.start = start;
        } else {
            step.text.push_str(removed);
        }
        step.end = after;
        self.bytes += step.cost();
        self.trim();
    }

    pub(super) fn has_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Reverses the last step. It returns false without a step.
    pub fn undo(&mut self, lines: &mut Lines, cursor: &mut Index2) -> bool {
        let Some(step) = self.undo.pop_back() else {
            return false;
        };
        self.bytes -= step.cost();
        let back = step.apply(lines, cursor);
        self.bytes += back.cost();
        self.redo.push_back(back);
        self.trim();
        true
    }

    /// Applies the last reversed step again. It returns false without a step.
    pub fn redo(&mut self, lines: &mut Lines, cursor: &mut Index2) -> bool {
        let Some(step) = self.redo.pop_back() else {
            return false;
        };
        self.bytes -= step.cost();
        let back = step.apply(lines, cursor);
        self.bytes += back.cost();
        self.undo.push_back(back);
        self.trim();
        true
    }

    /// Changes the limit. A lower limit removes the oldest steps now.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        self.trim();
    }

    /// Gives the newest steps to EdTUI, because the Vim mode uses its history. Each one
    /// becomes a full copy of the text there, and the reversed steps leave.
    pub fn move_to_edtui(&mut self, state: &mut EditorState) {
        self.clear_redo();
        // A reversed step can be large, and each step must stay until it applies again.
        let limit = std::mem::replace(&mut self.limit, usize::MAX);
        let count = self.undo.len().min(EDTUI_STEPS);
        for _ in 0..count {
            self.undo(&mut state.lines, &mut state.cursor);
        }
        for _ in 0..count {
            // `DeleteChar(0)` is the public way to record an undo point.
            state.execute(DeleteChar(0));
            self.redo(&mut state.lines, &mut state.cursor);
        }
        self.limit = limit;
        self.undo.clear();
        self.bytes = 0;
    }

    fn clear_redo(&mut self) {
        self.bytes -= self.redo.iter().map(Step::cost).sum::<usize>();
        self.redo.clear();
    }

    /// Removes the oldest steps. The newest step of each list always stays, so the last
    /// edit always has an undo.
    fn trim(&mut self) {
        while self.bytes > self.limit {
            let oldest = if self.undo.len() > 1 {
                self.undo.pop_front()
            } else if self.redo.len() > 1 {
                self.redo.pop_front()
            } else {
                None
            };
            let Some(step) = oldest else {
                return;
            };
            self.bytes -= step.cost();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl UndoHistory {
        fn counted(&self) -> usize {
            self.undo.iter().chain(&self.redo).map(Step::cost).sum()
        }
    }

    #[test]
    fn a_lower_limit_frees_the_oldest_steps_at_once() {
        let mut history = UndoHistory::default();
        let start = Index2::new(0, 0);
        for _ in 0..3 {
            history.push(start, start, "x".repeat(1000), start);
        }
        history.set_limit(1);
        assert_eq!(history.undo.len(), 1);
        assert_eq!(history.bytes, history.counted());
    }

    #[test]
    fn the_count_of_bytes_follows_each_operation() {
        let mut lines = Lines::from("abcdef");
        let mut cursor = Index2::new(0, 3);
        let mut history = UndoHistory::default();
        let at = |col| Index2::new(0, col);

        let end = replace_range(&mut lines, at(2), at(3), "");
        history.push(at(2), end, "c".to_owned(), cursor);
        assert_eq!(history.bytes, history.counted());
        let end = replace_range(&mut lines, at(1), at(2), "");
        history.join(at(1), at(2), "b", end);
        assert_eq!(history.bytes, history.counted());
        let end = replace_range(&mut lines, at(1), at(2), "");
        history.join(at(1), at(2), "d", end);
        assert_eq!(history.bytes, history.counted());

        cursor = end;
        assert!(history.undo(&mut lines, &mut cursor));
        assert_eq!(text_in(&lines, at(0), at(6)), "abcdef");
        assert_eq!(history.bytes, history.counted());
        assert!(history.redo(&mut lines, &mut cursor));
        assert_eq!(text_in(&lines, at(0), at(3)), "aef");
        assert_eq!(history.bytes, history.counted());

        assert!(history.undo(&mut lines, &mut cursor));
        history.clear_redo();
        assert_eq!(history.bytes, 0);
    }
}
