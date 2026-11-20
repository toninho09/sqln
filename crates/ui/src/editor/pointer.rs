//! Paste, clipboard copy and cut, and the mouse.

use super::*;

impl SqlEditor {
    /// Handles a paste from the terminal. It does not touch the clipboard.
    pub fn on_paste(&mut self, text: &str) {
        self.restore_scroll_selection();
        self.drop_list();
        self.status = None;
        let text = if self.literal_text {
            std::borrow::Cow::Borrowed(text)
        } else {
            std::borrow::Cow::Owned(normalize_newlines(text))
        };
        if let Some(search) = &mut self.search {
            let line = text.lines().next().unwrap_or_default();
            search.focused_text().push_str(line);
            self.after_search_text_change();
            return;
        }
        self.insert_at_cursor(&text);
    }

    pub(super) fn paste_from_clipboard(&mut self) {
        let text = self.clipboard.borrow_mut().paste();
        match text {
            Some(text) => {
                let text = if self.literal_text {
                    text
                } else {
                    normalize_newlines(&text)
                };
                self.insert_at_cursor(&text);
            }
            None => self.status = Some(text::CLIPBOARD_UNAVAILABLE),
        }
    }

    pub(super) fn copy_selection(&mut self) {
        let Some(text) = self.selected_text() else {
            return;
        };
        let outcome = self.clipboard.borrow_mut().copy(&text);
        self.status = match outcome {
            CopyOutcome::Copied => None,
            CopyOutcome::SentToTerminal => Some(text::COPY_SENT_TO_TERMINAL),
            CopyOutcome::Failed => Some(text::COPY_FAILED),
        };
    }

    pub(super) fn cut_selection(&mut self) {
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        self.copy_selection();
        if self.status != Some(text::COPY_FAILED) {
            self.edit(start, end, "", EditKind::Other);
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        if self.on_scrollbar_mouse(mouse) {
            self.drop_list();
            return;
        }
        self.drop_list();
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                self.handler.on_mouse_event(mouse, &mut self.state);
            }
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                let side = if matches!(mouse.kind, MouseEventKind::ScrollLeft) {
                    -1
                } else {
                    1
                };
                self.scroll_sideways(side);
            }
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(position) = self.pointer_position(mouse) {
                    self.scroll_selection_end = None;
                    match self.keyboard {
                        KeyboardMode::Conventional => self.point_conventional(mouse, position),
                        KeyboardMode::Vim => self.point_vim(mouse, position),
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn point_conventional(&mut self, mouse: MouseEvent, position: Index2) {
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.anchor = None;
        } else {
            self.anchor.get_or_insert(self.state.cursor);
        }
        self.state.cursor = position;
        self.preferred_col = None;
        self.break_undo_group();
        self.sync_selection();
    }

    pub(super) fn point_vim(&mut self, mouse: MouseEvent, position: Index2) {
        self.handler.on_mouse_event(mouse, &mut self.state);
        let last_col = buffer::row(&self.state.lines, position.row)
            .len()
            .saturating_sub(1);
        self.state.cursor = Index2::new(position.row, position.col.min(last_col));
        if let Some(selection) = &mut self.state.selection {
            selection.end = self.state.cursor;
        }
    }

    /// Moves the view sideways. EdTUI keeps the cursor inside the view at each draw,
    /// so the cursor moves when the view passes it. One event moves one column, because
    /// one gesture of a touchpad sends many events.
    pub(super) fn scroll_sideways(&mut self, columns: isize) {
        let width = self.text_area.map_or(0, |area| usize::from(area.width));
        if width == 0 {
            return;
        }
        let (old_left, top) = self.state.viewport_offset();
        let line = buffer::row(&self.state.lines, self.state.cursor.row);
        // The view cannot pass the last column that the cursor reaches on its row.
        let limit = match self.state.mode {
            EditorMode::Normal => line.len().saturating_sub(1),
            _ => line.len(),
        };
        let left = old_left.saturating_add_signed(columns).min(limit);
        let end = last_column(line, left, width).min(limit);
        let mut cursor = self.state.cursor;
        cursor.col = cursor.col.clamp(left, end);
        if cursor != self.state.cursor {
            self.state.cursor = cursor;
            self.preferred_col = None;
        }
        self.state.set_viewport_offset(left, top);
    }

    /// Converts a screen cell to a text position, with wide characters and tabs.
    /// EdTUI counts one column per character, and it stops at the last character.
    pub(super) fn pointer_position(&self, mouse: MouseEvent) -> Option<Index2> {
        let area = self.text_area?;
        if mouse.row < area.y || mouse.row >= area.bottom() || mouse.column >= area.right() {
            return None;
        }
        let (offset_x, offset_y) = self.state.viewport_offset();
        let row = usize::from(mouse.row - area.y) + offset_y;
        if row > buffer::last_row(&self.state.lines) {
            return Some(buffer::document_end(&self.state.lines));
        }
        let wanted = usize::from(mouse.column.saturating_sub(area.x));
        let chars = buffer::row(&self.state.lines, row);
        let mut width = 0;
        for (col, c) in chars.iter().enumerate().skip(offset_x) {
            width += if *c == '\t' {
                TAB_WIDTH
            } else {
                c.width().unwrap_or(0)
            };
            if width > wanted {
                return Some(Index2::new(row, col));
            }
        }
        Some(Index2::new(row, chars.len()))
    }
}
