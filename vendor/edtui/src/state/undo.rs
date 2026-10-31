//! This module contains a "brute-force" implementation of undo and redo.
//! It stores the entire editor state at each action.
//!
//! This approach works for basic undo/redo needs, but its not the most efficient.
//! In the long run, this should be replaced with an action-based mechanism.
use crate::{EditorState, Index2, Lines};

#[derive(Debug, Clone)]
pub(crate) struct Stack {
    inner: Vec<UndoState>,
    max_size: usize,
    bytes: usize,
}

impl Stack {
    pub(crate) fn new() -> Self {
        Self {
            inner: Vec::new(),
            max_size: 100,
            bytes: 0,
        }
    }

    pub(crate) fn pop(&mut self) -> Option<UndoState> {
        let value = self.inner.pop()?;
        self.bytes -= value.bytes();
        Some(value)
    }

    pub(crate) fn push(&mut self, value: UndoState) {
        self.bytes += value.bytes();
        self.inner.push(value);
        if self.len() > self.max_size {
            self.remove(0);
        }
    }

    fn len(&mut self) -> usize {
        self.inner.len()
    }

    fn remove(&mut self, index: usize) {
        let value = self.inner.remove(index);
        self.bytes -= value.bytes();
    }

    /// Removes the oldest entry. Returns false for an empty stack.
    fn remove_oldest(&mut self) -> bool {
        if self.inner.is_empty() {
            return false;
        }
        self.remove(0);
        true
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UndoState {
    lines: Lines,
    cursor: Index2,
}

impl UndoState {
    /// The memory of the text of this entry.
    fn bytes(&self) -> usize {
        let row = std::mem::size_of::<Vec<char>>();
        let letter = std::mem::size_of::<char>();
        self.lines.iter_row().map(|line| row + line.len() * letter).sum()
    }
}

impl EditorState {
    pub(crate) fn capture(&mut self) {
        let editor_state = UndoState {
            lines: self.lines.clone(),
            cursor: self.cursor,
        };
        // An entry above the budget stays out, and the owner can tell the user.
        if editor_state.bytes() > self.undo_limit {
            self.undo_skipped = true;
            return;
        }
        self.undo.push(editor_state);
        self.trim_history();
    }

    /// Limits the memory of the undo and redo stacks together.
    /// The oldest entries leave first.
    pub fn set_undo_limit(&mut self, bytes: usize) {
        self.undo_limit = bytes;
        self.trim_history();
    }

    /// Returns true once after an edit that was too large for the history.
    pub fn take_undo_skipped(&mut self) -> bool {
        std::mem::take(&mut self.undo_skipped)
    }

    fn trim_history(&mut self) {
        while self.undo.bytes + self.redo.bytes > self.undo_limit {
            if !self.undo.remove_oldest() && !self.redo.remove_oldest() {
                return;
            }
        }
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            let current = UndoState {
                lines: self.lines.clone(),
                cursor: self.cursor,
            };
            self.lines = prev.lines;
            self.cursor = prev.cursor;
            self.redo.push(current);
            self.trim_history();
        }
    }

    pub fn redo(&mut self) {
        if let Some(prev) = self.redo.pop() {
            let current = UndoState {
                lines: self.lines.clone(),
                cursor: self.cursor,
            };
            self.lines = prev.lines;
            self.cursor = prev.cursor;
            self.undo.push(current);
            self.trim_history();
        }
    }
}
