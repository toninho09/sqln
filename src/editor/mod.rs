//! The SQL editor: EdTUI, the conventional keys, suggestions, search, and highlights.

mod buffer;
mod conventional;
mod search;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use edtui::actions::{DeleteChar, SwitchMode};
use edtui::{EditorEventHandler, EditorMode, EditorState, Highlight, Index2, Lines};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

use self::buffer::{Snapshot, normalize_newlines, replace_range};
use self::conventional::{EditKind, INDENT, UndoGroup};
pub use self::search::{Match, SearchBar, SearchField};
use crate::catalog::{Catalog, RelationRef};
use crate::clipboard::{CopyOutcome, EditorClipboard, SharedClipboard};
use crate::complete::{Completion, keywords, suggest_in};
use crate::sql::{Dialect, TokenKind, split, split_tokens, statement_at};
use crate::ui::text;
use crate::ui::theme::Theme;

pub const TAB_WIDTH: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyboardMode {
    #[default]
    Conventional,
    Vim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunScope {
    /// The selection or, without a selection, the statement at the cursor.
    Default,
    Script,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunRefusal {
    CursorOutside,
    NoStatement,
}

/// The statements of one execution. It holds a copy of the text, so later edits
/// do not change a running execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunPlan {
    pub statements: Vec<String>,
    pub rows: Vec<usize>,
    /// The byte offset of each statement in the text of the editor.
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkState {
    Running,
    Success,
    Capped,
    Error,
    NotExecuted,
    Stopped,
    Timeout,
    Unknown,
}

/// The state of one statement in the margin. An edit of the statement removes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementMark {
    pub index: usize,
    pub row: usize,
    pub start: usize,
    pub text: String,
    pub state: Option<MarkState>,
}

/// The part of the text that a database error names. It describes one revision.
struct ErrorSpot {
    start: usize,
    end: usize,
    revision: u64,
}

/// The open list of suggestions. The editor rebuilds it after each text change.
pub struct SuggestionList {
    pub completion: Completion,
    pub selected: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListAction {
    Open,
    Refresh,
    Close,
}

pub struct SqlEditor {
    pub state: EditorState,
    keyboard: KeyboardMode,
    handler: EditorEventHandler,
    dialect: Dialect,
    clipboard: SharedClipboard,
    anchor: Option<Index2>,
    preferred_col: Option<usize>,
    undo_group: Option<UndoGroup>,
    redo_depth: usize,
    page_height: usize,
    text_area: Option<Rect>,
    revision: u64,
    /// The last known text in Vim mode. EdTUI does not report its edits.
    vim_shadow: Option<Lines>,
    snapshot: Option<(u64, Snapshot)>,
    list: Option<SuggestionList>,
    search: Option<SearchBar>,
    status: Option<&'static str>,
    missing_columns: Vec<RelationRef>,
    /// The position of the last request for a list, for a catalog that arrives later.
    list_request: Option<(u64, Index2)>,
    marks: Vec<StatementMark>,
    marks_revision: u64,
    /// The revision of the text that the marks of the execution describe.
    plan_revision: u64,
    error_spot: Option<ErrorSpot>,
}

impl SqlEditor {
    pub fn new(
        text: &str,
        keyboard: KeyboardMode,
        dialect: Dialect,
        clipboard: SharedClipboard,
    ) -> Self {
        let mut state = EditorState::new(Lines::from(text));
        state.set_clipboard(EditorClipboard(clipboard.clone()));
        let mut lines = std::mem::take(&mut state.lines).into_vecs();
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        state.lines = Lines::new(lines);
        if keyboard == KeyboardMode::Conventional {
            state.mode = EditorMode::Insert;
        }
        let vim_shadow = (keyboard == KeyboardMode::Vim).then(|| state.lines.clone());
        Self {
            state,
            keyboard,
            handler: EditorEventHandler::vim_mode(),
            dialect,
            clipboard,
            anchor: None,
            preferred_col: None,
            undo_group: None,
            redo_depth: 0,
            page_height: 20,
            text_area: None,
            revision: 0,
            vim_shadow,
            snapshot: None,
            list: None,
            search: None,
            status: None,
            missing_columns: Vec::new(),
            list_request: None,
            marks: Vec::new(),
            marks_revision: 0,
            plan_revision: 0,
            error_spot: None,
        }
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Changes the SQL rules of the text. The next frame reads the text again.
    pub fn set_dialect(&mut self, dialect: Dialect) {
        self.dialect = dialect;
        self.snapshot = None;
    }

    pub fn keyboard(&self) -> KeyboardMode {
        self.keyboard
    }

    pub fn text(&self) -> String {
        buffer::text_of(&self.state.lines)
    }

    pub fn cursor(&self) -> Index2 {
        self.state.cursor
    }

    /// Returns a number that changes with each change of the text.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the first visible column and row.
    pub fn scroll(&self) -> (usize, usize) {
        self.state.viewport_offset()
    }

    pub fn set_scroll(&mut self, column: usize, row: usize) {
        self.state.set_viewport_offset(column, row);
    }

    pub fn set_cursor(&mut self, cursor: Index2) {
        self.state.cursor = cursor;
        self.anchor = None;
        self.break_undo_group();
        self.sync_selection();
    }

    pub fn selected_text(&self) -> Option<String> {
        match self.keyboard {
            KeyboardMode::Conventional => self.selected_text_conventional(),
            KeyboardMode::Vim => {
                let selection = self.state.selection.as_ref()?;
                Some(selection.copy_from(&self.state.lines).to_string())
            }
        }
    }

    pub fn suggestions(&self) -> Option<&SuggestionList> {
        self.list.as_ref()
    }

    pub fn search(&self) -> Option<&SearchBar> {
        self.search.as_ref()
    }

    pub fn status(&self) -> Option<&'static str> {
        self.status
    }

    /// Returns the relations that need a catalog read, and it clears the list.
    pub fn take_missing_columns(&mut self) -> Vec<RelationRef> {
        std::mem::take(&mut self.missing_columns)
    }

    fn mark_changed(&mut self) {
        self.revision += 1;
    }

    fn snapshot(&mut self) -> &Snapshot {
        let revision = self.revision;
        let current = self.snapshot.take_if(|(cached, _)| *cached == revision);
        let cached = current
            .unwrap_or_else(|| (revision, Snapshot::new(&self.state.lines, self.dialect)));
        &self.snapshot.insert(cached).1
    }

    /// Handles a key for the open list. It returns false when other handlers must act.
    pub fn on_list_key(&mut self, key: KeyEvent) -> bool {
        let Some(list) = &mut self.list else { return false };
        let count = list.completion.items.len();
        let plain = key.modifiers.is_empty();
        match key.code {
            KeyCode::Tab if plain => {
                self.accept_suggestion();
                true
            }
            KeyCode::Esc => {
                self.list = None;
                self.list_request = None;
                true
            }
            KeyCode::Up if plain => {
                list.selected = (list.selected + count - 1) % count;
                true
            }
            KeyCode::Down if plain => {
                list.selected = (list.selected + 1) % count;
                true
            }
            _ => false,
        }
    }

    fn accept_suggestion(&mut self) {
        let Some(list) = self.list.take() else { return };
        let Some(item) = list.completion.items.get(list.selected) else { return };
        let snapshot = self.snapshot();
        let start = snapshot.position(list.completion.replace.start);
        let end = snapshot.position(list.completion.replace.end);
        self.break_undo_group();
        self.edit(start, end, &item.insert, EditKind::Other);
    }

    pub fn on_key(&mut self, key: KeyEvent, catalog: Option<&Catalog>) {
        self.status = None;
        if self.search.is_some() {
            self.on_search_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(' ') if ctrl => {
                if self.state.mode == EditorMode::Insert {
                    self.update_list(ListAction::Open, catalog);
                }
                return;
            }
            KeyCode::Char('f') if ctrl => {
                self.open_search();
                return;
            }
            KeyCode::Char('/')
                if self.keyboard == KeyboardMode::Vim && self.state.mode == EditorMode::Normal =>
            {
                self.open_search();
                return;
            }
            _ => {}
        }
        let action = self.list_action(key);
        match self.keyboard {
            KeyboardMode::Conventional => self.on_conventional_key(key),
            KeyboardMode::Vim => self.on_vim_key(key),
        }
        self.update_list(action, catalog);
    }

    /// Decides the effect of a key on the list before the editor applies it.
    fn list_action(&self, key: KeyEvent) -> ListAction {
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

    fn update_list(&mut self, action: ListAction, catalog: Option<&Catalog>) {
        if action == ListAction::Close || (action == ListAction::Refresh && self.list.is_none()) {
            self.list = None;
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
        self.list = (!completion.items.is_empty())
            .then_some(SuggestionList { completion, selected: 0 });
    }

    pub fn close_suggestions(&mut self) {
        self.list = None;
        self.list_request = None;
    }

    /// Rebuilds the list after the catalog receives data. The list also opens
    /// when a request found nothing and the text and the cursor did not change.
    pub fn refresh_suggestions(&mut self, catalog: Option<&Catalog>) {
        if self.list.is_some() || self.list_request == Some((self.revision, self.state.cursor)) {
            self.update_list(ListAction::Open, catalog);
        }
    }

    fn selection_range(&self) -> Option<(Index2, Index2)> {
        match self.keyboard {
            KeyboardMode::Conventional => self.selected_range(),
            KeyboardMode::Vim => self.state.selection.is_some().then(|| self.replace_target()),
        }
    }

    /// Finds the statements of an execution, and it copies their text.
    pub fn plan_run(&mut self, scope: RunScope) -> Result<RunPlan, RunRefusal> {
        let dialect = self.dialect;
        let selection = (scope == RunScope::Default).then(|| self.selection_range()).flatten();
        let cursor = self.state.cursor;
        let snapshot = self.snapshot();
        let text = snapshot.text.as_str();

        let (base, found) = if let Some((start, end)) = selection {
            let (start, end) = (snapshot.offset(start), snapshot.offset(end));
            (start, split(&text[start..end], dialect))
        } else if scope == RunScope::Script {
            (0, split(text, dialect))
        } else {
            let statement = statement_at(text, dialect, snapshot.offset(cursor));
            (0, vec![statement.ok_or(RunRefusal::CursorOutside)?])
        };
        if found.is_empty() {
            return Err(RunRefusal::NoStatement);
        }
        let source = &text[base..];
        Ok(RunPlan {
            statements: found.iter().map(|found| found.text(source).to_owned()).collect(),
            rows: found.iter().map(|found| snapshot.position(base + found.start).row).collect(),
            starts: found.iter().map(|found| base + found.start).collect(),
        })
    }

    pub fn set_marks(&mut self, plan: &RunPlan) {
        self.marks = plan
            .statements
            .iter()
            .zip(plan.rows.iter().zip(&plan.starts))
            .enumerate()
            .map(|(index, (text, (row, start)))| StatementMark {
                index,
                row: *row,
                start: *start,
                text: text.clone(),
                state: None,
            })
            .collect();
        self.marks_revision = self.revision;
        self.plan_revision = self.revision;
        self.error_spot = None;
    }

    /// Marks the token of a database error. A text that changed after the start of
    /// the execution has no reliable place for it, so the editor marks nothing.
    pub fn set_error_offset(&mut self, index: usize, offset: usize) {
        if self.revision != self.plan_revision {
            return;
        }
        let Some(mark) = self.marks.iter().find(|mark| mark.index == index) else { return };
        let (at, statement_end) = (mark.start + offset, mark.start + mark.text.len());
        let revision = self.revision;
        let tokens = &self.snapshot().tokens;
        let next = tokens.partition_point(|token| token.end <= at);
        let token = tokens
            .get(next)
            .filter(|token| token.start <= at && at < statement_end)
            .or_else(|| tokens[..next].last());
        self.error_spot =
            token.map(|token| ErrorSpot { start: token.start, end: token.end, revision });
    }

    pub fn set_mark_state(&mut self, index: usize, state: MarkState) {
        if let Some(mark) = self.marks.iter_mut().find(|mark| mark.index == index) {
            mark.state = Some(state);
        }
    }

    /// Returns the marks of the current text. A statement with another text loses
    /// its mark, because an old result must not describe the new text.
    pub fn marks(&mut self) -> &[StatementMark] {
        if self.marks_revision != self.revision && !self.marks.is_empty() {
            self.marks_revision = self.revision;
            let dialect = self.dialect;
            let mut marks = std::mem::take(&mut self.marks);
            let snapshot = self.snapshot();
            let text = snapshot.text.as_str();
            let statements = split(text, dialect);
            let mut used = vec![false; statements.len()];
            marks.retain_mut(|mark| {
                let nearest = statements
                    .iter()
                    .enumerate()
                    .filter(|(index, statement)| !used[*index] && statement.text(text) == mark.text)
                    .map(|(index, statement)| (index, snapshot.position(statement.start).row))
                    .min_by_key(|(_, row)| row.abs_diff(mark.row));
                if let Some((index, row)) = nearest {
                    used[index] = true;
                    mark.row = row;
                }
                nearest.is_some()
            });
            self.marks = marks;
        }
        &self.marks
    }

    fn on_vim_key(&mut self, key: KeyEvent) {
        let mode = self.state.mode;
        let plain = key.modifiers.is_empty();
        let ctrl = key.modifiers == KeyModifiers::CONTROL;
        if mode == EditorMode::Insert && key.code == KeyCode::Tab && plain {
            self.insert_at_cursor(INDENT);
            return;
        }
        let is_redo = mode == EditorMode::Normal && ctrl && key.code == KeyCode::Char('r');
        if is_redo && self.redo_depth == 0 {
            return;
        }
        let is_undo = mode == EditorMode::Normal && plain && key.code == KeyCode::Char('u');

        self.handler.on_key_event(key, &mut self.state);
        if !self.sync_vim_shadow() {
            return;
        }
        if is_undo {
            self.redo_depth += 1;
        } else if is_redo {
            self.redo_depth -= 1;
        } else {
            self.redo_depth = 0;
        }
    }

    /// Returns true when EdTUI changed the text.
    fn sync_vim_shadow(&mut self) -> bool {
        if self.vim_shadow.as_ref() == Some(&self.state.lines) {
            return false;
        }
        self.vim_shadow = Some(self.state.lines.clone());
        self.mark_changed();
        true
    }

    /// Inserts text at the cursor as one undo step, without the clipboard.
    fn insert_at_cursor(&mut self, text: &str) {
        self.break_undo_group();
        let (start, end) = self.replace_target();
        self.edit(start, end, text, EditKind::Other);
        self.leave_visual_mode();
        if self.keyboard == KeyboardMode::Vim && self.state.mode == EditorMode::Normal {
            let cursor = self.state.cursor;
            let last_col = buffer::row(&self.state.lines, cursor.row).len().saturating_sub(1);
            self.state.cursor.col = cursor.col.min(last_col);
        }
    }

    fn replace_target(&self) -> (Index2, Index2) {
        if self.keyboard == KeyboardMode::Conventional {
            let cursor = self.state.cursor;
            return self.selected_range().unwrap_or((cursor, cursor));
        }
        let Some(selection) = &self.state.selection else {
            let cursor = self.state.cursor;
            let col = cursor.col.min(buffer::row(&self.state.lines, cursor.row).len());
            return (Index2::new(cursor.row, col), Index2::new(cursor.row, col));
        };
        let end = selection.end();
        let row_len = buffer::row(&self.state.lines, end.row).len();
        let after = if end.col >= row_len {
            buffer::next_position(&self.state.lines, Index2::new(end.row, row_len))
        } else {
            Index2::new(end.row, end.col + 1)
        };
        (selection.start(), after)
    }

    fn leave_visual_mode(&mut self) {
        if self.keyboard == KeyboardMode::Vim && self.state.mode == EditorMode::Visual {
            self.state.execute(SwitchMode(EditorMode::Normal));
        }
        self.sync_vim_shadow();
    }

    /// Handles a paste from the terminal. It does not touch the clipboard.
    pub fn on_paste(&mut self, text: &str) {
        self.list = None;
        self.status = None;
        let text = normalize_newlines(text);
        if let Some(search) = &mut self.search {
            let line = text.lines().next().unwrap_or_default();
            search.focused_text().push_str(line);
            self.after_search_text_change();
            return;
        }
        self.insert_at_cursor(&text);
    }

    fn paste_from_clipboard(&mut self) {
        let text = self.clipboard.borrow_mut().paste();
        match text {
            Some(text) => self.insert_at_cursor(&normalize_newlines(&text)),
            None => self.status = Some(text::CLIPBOARD_UNAVAILABLE),
        }
    }

    fn copy_selection(&mut self) {
        let Some(text) = self.selected_text() else { return };
        let outcome = self.clipboard.borrow_mut().copy(&text);
        self.status = match outcome {
            CopyOutcome::Copied => None,
            CopyOutcome::SentToTerminal => Some(text::COPY_SENT_TO_TERMINAL),
            CopyOutcome::Failed => Some(text::COPY_FAILED),
        };
    }

    fn cut_selection(&mut self) {
        let Some((start, end)) = self.selected_range() else { return };
        self.copy_selection();
        if self.status != Some(text::COPY_FAILED) {
            self.edit(start, end, "", EditKind::Other);
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        self.list = None;
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                self.handler.on_mouse_event(mouse, &mut self.state);
            }
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(position) = self.pointer_position(mouse) {
                    match self.keyboard {
                        KeyboardMode::Conventional => self.point_conventional(mouse, position),
                        KeyboardMode::Vim => self.point_vim(mouse, position),
                    }
                }
            }
            _ => {}
        }
    }

    fn point_conventional(&mut self, mouse: MouseEvent, position: Index2) {
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

    fn point_vim(&mut self, mouse: MouseEvent, position: Index2) {
        self.handler.on_mouse_event(mouse, &mut self.state);
        let last_col = buffer::row(&self.state.lines, position.row).len().saturating_sub(1);
        self.state.cursor = Index2::new(position.row, position.col.min(last_col));
        if let Some(selection) = &mut self.state.selection {
            selection.end = self.state.cursor;
        }
    }

    /// Converts a screen cell to a text position, with wide characters and tabs.
    /// EdTUI counts one column per character, and it stops at the last character.
    fn pointer_position(&self, mouse: MouseEvent) -> Option<Index2> {
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
            width += if *c == '\t' { TAB_WIDTH } else { c.width().unwrap_or(0) };
            if width > wanted {
                return Some(Index2::new(row, col));
            }
        }
        Some(Index2::new(row, chars.len()))
    }

    fn open_search(&mut self) {
        let mut bar = SearchBar::default();
        if let Some(selected) = self.selected_text().filter(|text| !text.contains('\n')) {
            bar.find = selected;
        }
        self.leave_visual_mode();
        bar.refresh(&self.state.lines, self.state.cursor);
        self.list = None;
        self.search = Some(bar);
        self.move_to_current_match();
    }

    fn move_to_current_match(&mut self) {
        let found = self.search.as_ref().and_then(SearchBar::current_match);
        if let Some(found) = found {
            self.set_cursor(found.start);
        }
    }

    fn after_search_text_change(&mut self) {
        let cursor = self.state.cursor;
        if let Some(search) = &mut self.search {
            search.refresh(&self.state.lines, cursor);
        }
        self.move_to_current_match();
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        let Some(search) = &mut self.search else { return };
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

    fn replace_current(&mut self) {
        let Some(search) = &self.search else { return };
        let Some(found) = search.current_match() else { return };
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

    fn replace_all(&mut self) {
        let Some(search) = &self.search else { return };
        if search.matches.is_empty() {
            return;
        }
        let matches = search.matches.clone();
        let replacement = search.replace.clone();
        self.state.execute(DeleteChar(0));
        let lines = &mut self.state.lines;
        for found in matches.iter().rev() {
            replace_range(lines, found.start, found.end, &replacement);
        }
        let first = matches[0].start;
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

    /// Writes the highlights of the rows that the next frame shows.
    pub fn prepare_frame(&mut self, height: usize, theme: &Theme) {
        self.page_height = height;
        let cursor_row = self.state.cursor.row;
        let (_, top) = self.state.viewport_offset();
        let top = if cursor_row < top {
            cursor_row
        } else if cursor_row >= top + height {
            cursor_row + 1 - height
        } else {
            top
        };
        let dialect = self.dialect;
        let mut highlights = self.search_highlights(top, height, theme);
        let spot = self
            .error_spot
            .as_ref()
            .filter(|spot| spot.revision == self.revision)
            .map(|spot| (spot.start, spot.end));
        let snapshot = self.snapshot();
        if let Some((start, end)) = spot {
            let start = snapshot.position(start);
            highlights.push(Highlight::new(start, snapshot.position_before(end), theme.error_spot));
        }
        let visible_start = snapshot.line_start(top);
        let visible_end = snapshot.line_start(top + height + 1);
        let first = snapshot.tokens.partition_point(|token| token.end <= visible_start);
        for token in &snapshot.tokens[first..] {
            if token.start >= visible_end {
                break;
            }
            let word = &snapshot.text[token.start..token.end];
            let style = match token.kind {
                TokenKind::Word if keywords::is_keyword(word, dialect) => theme.keyword,
                TokenKind::Number => theme.number,
                TokenKind::String => theme.string,
                TokenKind::QuotedIdentifier => theme.identifier,
                TokenKind::Comment => theme.comment,
                _ => continue,
            };
            let start = snapshot.position(token.start);
            let end = snapshot.position_before(token.end);
            highlights.push(Highlight::new(start, end, style));
        }
        self.state.set_highlights(highlights);
    }

    fn search_highlights(&self, top: usize, height: usize, theme: &Theme) -> Vec<Highlight> {
        let Some(search) = &self.search else { return Vec::new() };
        let current = search.current_match();
        let first = search.matches.partition_point(|found| found.start.row < top);
        search.matches[first..]
            .iter()
            .take_while(|found| found.start.row <= top + height)
            .map(|found| {
                let style =
                    if Some(*found) == current { theme.search_current } else { theme.search_match };
                let end = Index2::new(found.end.row, found.end.col - 1);
                Highlight::new(found.start, end, style)
            })
            .collect()
    }

    pub fn set_text_area(&mut self, area: Rect) {
        self.text_area = Some(area);
    }

    /// Returns the characters between the start of the partial name and the cursor.
    pub fn list_anchor_chars(&mut self) -> usize {
        let cursor = self.state.cursor;
        let Some(list) = &self.list else { return 0 };
        let start = list.completion.replace.start;
        let snapshot = self.snapshot();
        let offset = snapshot.offset(cursor).max(start);
        snapshot.text[start..offset].chars().count()
    }
}
