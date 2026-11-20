//! The suggestion list: keys, acceptance, refresh, and the wait for columns.

use super::*;

impl SqlEditor {
    /// Handles a key for the open list. It returns false when other handlers must act.
    pub fn on_list_key(&mut self, key: KeyEvent, keys: SuggestionKeys) -> bool {
        let Some(list) = &mut self.list else {
            return false;
        };
        if keys.accepts(key) {
            if list.ai && list.selected < AI_ROWS {
                self.ai_requested = Some(if list.selected == 0 {
                    self::ai::AiSelection::Request
                } else {
                    self::ai::AiSelection::Plain
                });
                self.close_suggestions();
                return true;
            }
            self.accept_suggestion();
            return true;
        }
        let total = ai_row_count(list.ai) + list.completion.items.len();
        let plain = key.modifiers.is_empty();
        match key.code {
            KeyCode::Esc => {
                self.drop_list();
                self.list_request = None;
                true
            }
            KeyCode::Up if plain && total > 0 => {
                list.selected = (list.selected + total - 1) % total;
                true
            }
            KeyCode::Down if plain && total > 0 => {
                list.selected = (list.selected + 1) % total;
                true
            }
            _ => false,
        }
    }

    pub(super) fn accept_suggestion(&mut self) {
        let Some(list) = self.list.take() else { return };
        let index = list.selected.saturating_sub(ai_row_count(list.ai));
        let Some(item) = list.completion.items.get(index) else {
            return;
        };
        let snapshot = self.snapshot();
        let start = snapshot.position(list.completion.replace.start);
        let end = snapshot.position(list.completion.replace.end);
        self.break_undo_group();
        self.edit(start, end, &item.insert, EditKind::Other);
        if let Some(offset) = item.cursor_offset {
            let cursor = self
                .snapshot()
                .position(list.completion.replace.start + offset);
            self.set_cursor(cursor);
        }
    }

    /// Decides the effect of a key on the list before the editor applies it.
    pub(super) fn list_action(&self, key: KeyEvent) -> ListAction {
        if self.state.mode != EditorMode::Insert
            || !key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            return ListAction::Close;
        }
        match key.code {
            KeyCode::Char(c) if c.is_alphanumeric() || matches!(c, '_' | '.' | '"' | '`' | '[') => {
                ListAction::Open
            }
            KeyCode::Backspace | KeyCode::Delete => ListAction::Refresh,
            _ => ListAction::Close,
        }
    }

    /// Keeps database value bytes, including CRLF, instead of normalizing script line endings.
    pub fn use_literal_text(&mut self, text: &str) {
        self.literal_text = true;
        self.state.lines = Lines::new(
            text.split('\n')
                .map(|line| line.chars().collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        );
        self.snapshot = None;
        self.sync_vim_shadow();
        self.disable_suggestions();
    }

    pub fn disable_suggestions(&mut self) {
        self.suggestions_enabled = false;
        self.close_suggestions();
    }

    pub(super) fn update_list(&mut self, action: ListAction, catalog: Option<&Catalog>) {
        if !self.suggestions_enabled {
            self.drop_list();
            return;
        }
        let open = self.list.is_some() || self.waiting.is_some();
        if action == ListAction::Close || (action == ListAction::Refresh && !open) {
            self.drop_list();
            return;
        }
        let cursor = self.state.cursor;
        let dialect = self.dialect;
        self.list_request = Some((self.revision, cursor));
        let snapshot = self.snapshot();
        let statements = split_tokens(&snapshot.text, dialect, &snapshot.tokens);
        let offset = snapshot.offset(cursor);
        let completion = suggest_in(&snapshot.text, &statements, offset, dialect, catalog);
        for relation in &completion.missing_columns {
            if !self.missing_columns.contains(relation) {
                self.missing_columns.push(relation.clone());
            }
        }
        self.waiting = completion
            .missing_columns
            .first()
            .filter(|_| completion.items.is_empty())
            .cloned();
        self.list = (!completion.items.is_empty()).then_some(SuggestionList {
            completion,
            // The list starts at the first local item; the AI rows wait above it.
            selected: ai_row_count(self.ai_row()),
            ai: self.ai_row(),
        });
    }

    pub fn close_suggestions(&mut self) {
        self.list_area = Default::default();
        self.drop_list();
        self.list_request = None;
    }

    pub(super) fn drop_list(&mut self) {
        self.list = None;
        self.waiting = None;
    }

    /// Returns the relation whose columns the list waits for.
    pub fn waiting(&self) -> Option<&str> {
        self.waiting
            .as_ref()
            .map(|relation| relation.relation.as_str())
    }

    /// Ends the wait for a relation after a failed read of its columns. A later catalog
    /// does not open the list for the same request.
    pub fn stop_waiting(&mut self, relation: &RelationRef) {
        if self.waiting.as_ref() == Some(relation) {
            self.waiting = None;
            self.list_request = None;
        }
    }

    /// Rebuilds the list after the catalog receives data. The list also opens
    /// when a request found nothing and the text and the cursor did not change.
    pub fn refresh_suggestions(&mut self, catalog: Option<&Catalog>) {
        if self.list.is_some() || self.list_request == Some((self.revision, self.state.cursor)) {
            let ai_selected = self
                .list
                .as_ref()
                .and_then(|l| l.ai.then_some(l.selected))
                .filter(|s| *s < AI_ROWS);
            self.update_list(ListAction::Open, catalog);
            if let Some(index) = ai_selected.filter(|_| self.ai_available) {
                let list = self.list.get_or_insert(SuggestionList {
                    completion: Completion::default(),
                    selected: index,
                    ai: true,
                });
                list.selected = index;
            }
        }
    }
}
