//! Search and replace in the editor text.

use super::*;

impl SqlEditor {
    pub(super) fn open_search(&mut self) {
        let mut bar = SearchBar::default();
        if let Some(selected) = self.selected_text().filter(|text| !text.contains('\n')) {
            bar.find = selected;
        }
        self.leave_visual_mode();
        bar.refresh(&self.state.lines, self.state.cursor);
        self.drop_list();
        self.search = Some(bar);
        self.move_to_current_match();
    }

    pub(super) fn move_to_current_match(&mut self) {
        let found = self.search.as_ref().and_then(SearchBar::current_match);
        if let Some(found) = found {
            self.set_cursor(found.start);
        }
    }

    pub(super) fn after_search_text_change(&mut self) {
        let cursor = self.state.cursor;
        if let Some(search) = &mut self.search {
            search.refresh(&self.state.lines, cursor);
        }
        self.move_to_current_match();
    }

    pub(super) fn on_search_key(&mut self, key: KeyEvent) {
        let Some(search) = &mut self.search else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.search = None;
                self.state.clear_highlights();
            }
            KeyCode::Tab | KeyCode::BackTab => search.toggle_field(),
            KeyCode::Down => {
                search.advance(true);
                self.move_to_current_match();
            }
            KeyCode::Up => {
                search.advance(false);
                self.move_to_current_match();
            }
            KeyCode::Enter if search.field == SearchField::Find => {
                search.advance(true);
                self.move_to_current_match();
            }
            KeyCode::Enter => self.replace_current(),
            KeyCode::Char('a') if ctrl && search.field == SearchField::Replace => {
                self.replace_all();
            }
            KeyCode::Backspace => {
                search.focused_text().pop();
                self.after_search_text_change();
            }
            KeyCode::Char(c) if !ctrl => {
                search.focused_text().push(c);
                if search.field == SearchField::Find {
                    self.after_search_text_change();
                }
            }
            _ => {}
        }
    }

    pub(super) fn replace_current(&mut self) {
        let Some(search) = &self.search else { return };
        let Some(found) = search.current_match() else {
            return;
        };
        let replacement = search.replace.clone();
        self.break_undo_group();
        self.edit(found.start, found.end, &replacement, EditKind::Other);
        self.sync_vim_shadow();
        let cursor = self.state.cursor;
        if let Some(search) = &mut self.search {
            search.refresh(&self.state.lines, cursor);
        }
        self.move_to_current_match();
    }

    pub(super) fn replace_all(&mut self) {
        let Some(search) = &self.search else { return };
        if search.matches.is_empty() {
            return;
        }
        let replacement = search.replace.clone();
        let ranges: Vec<_> = search
            .matches
            .iter()
            .map(|found| (found.start, found.end, replacement.as_str()))
            .collect();
        self.replace_ranges(&ranges);
    }

    /// Replaces ranges as one undo step. The ranges come in the order of the text, and
    /// the cursor goes to the start of the first one.
    pub(super) fn replace_ranges(&mut self, ranges: &[(Index2, Index2, &str)]) {
        self.scroll_selection_end = None;
        match self.keyboard {
            KeyboardMode::Conventional => self.replace_ranges_in_one_step(ranges),
            KeyboardMode::Vim => {
                self.state.execute(DeleteChar(0));
                replace_ranges(&mut self.state.lines, ranges);
            }
        }
        let first = ranges[0].0;
        self.state.cursor = Index2::new(first.row, first.col);
        self.anchor = None;
        self.redo_depth = 0;
        self.break_undo_group();
        self.sync_selection();
        self.mark_changed();
        self.sync_vim_shadow();
        if let Some(search) = &mut self.search {
            search.refresh(&self.state.lines, first);
        }
    }

    /// Records one step from the start of the first range to the end of the last one.
    /// The text after the last range does not change, so it gives the end of the step.
    pub(super) fn replace_ranges_in_one_step(&mut self, ranges: &[(Index2, Index2, &str)]) {
        let (start, last) = (ranges[0].0, ranges[ranges.len() - 1].1);
        let lines = &mut self.state.lines;
        let removed = buffer::text_in(lines, start, last);
        let rows_after = lines.len() - last.row;
        let columns_after = buffer::row(lines, last.row).len() - last.col;
        replace_ranges(lines, ranges);
        let row = lines.len() - rows_after;
        let end = Index2::new(row, buffer::row(lines, row).len() - columns_after);
        self.history.push(start, end, removed, self.state.cursor);
    }

    pub(super) fn mark_search(&self, window: &Window, styles: &mut Styles, theme: &Theme) {
        let Some(search) = &self.search else {
            return;
        };
        let current = search.current_match();
        let first = search
            .matches
            .partition_point(|found| found.start.row < window.top);
        let shown = search.matches[first..]
            .iter()
            .take_while(|found| found.start.row < window.top + window.ends.len());
        for found in shown {
            let style = if Some(*found) == current {
                theme.search_current
            } else {
                theme.search_match
            };
            let end = Index2::new(found.end.row, found.end.col - 1);
            styles.mark(found.start, end, style);
        }
    }
}
