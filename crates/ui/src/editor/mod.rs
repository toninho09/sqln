//! The SQL editor: EdTUI, the conventional keys, suggestions, search, and highlights.

mod agent;
pub mod ai;
mod buffer;
mod conventional;
mod frame;
pub mod review;
mod run;
pub use run::RunPreview;
mod scroll;
mod search;
pub use scroll::Scrollbar;
mod syntax;
mod undo;

use std::collections::{BTreeSet, HashMap};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use edtui::actions::{DeleteChar, SwitchMode};
use edtui::{EditorEventHandler, EditorMode, EditorState, Index2, Lines, RowIndex};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

use self::buffer::{Snapshot, normalize_newlines, replace_ranges, toggle_comment};
use self::conventional::{EditKind, INDENT, UndoGroup};
pub use self::frame::FrameState;
use self::frame::{Styles, Window, window_end};
pub use self::search::{Match, SearchBar, SearchField};
pub use self::undo::DEFAULT_UNDO_LIMIT;
use self::undo::UndoHistory;
use crate::catalog::{Catalog, RelationRef};
use crate::clipboard::{CopyOutcome, EditorClipboard, SharedClipboard};
use crate::complete::{Completion, suggest_in};
pub use crate::input::KeyboardMode;
use crate::input::{Action, SuggestionKeys};
use crate::sql::{Dialect, format_statement, split, split_tokens};
use crate::ui::text;
use crate::ui::theme::Theme;

mod marks;
mod pointer;
mod replace;
mod suggestions;

pub const TAB_WIDTH: usize = 4;

/// Returns the `(row, index)` nearest to a row. A tie takes the lowest index.
fn nearest_place(places: &BTreeSet<(usize, usize)>, row: usize) -> Option<(usize, usize)> {
    let first_on = |row: usize| places.range((row, 0)..).next().copied();
    let before = places.range(..(row, 0)).next_back();
    let before = before.and_then(|(row, _)| first_on(*row));
    match (before, first_on(row)) {
        (Some(before), Some(after)) if row - before.0 <= after.0 - row => Some(before),
        (before, after) => after.or(before),
    }
}

/// Returns the cells of a character on the screen of the editor.
fn cells(c: char) -> usize {
    match c {
        '\t' => TAB_WIDTH,
        c => c.width().unwrap_or(0),
    }
}

/// Returns the last column that a cursor reaches in `width` cells from `left`.
fn last_column(line: &[char], left: usize, width: usize) -> usize {
    let mut used = 0;
    let rest = line.get(left..).unwrap_or_default();
    let fitting = rest.iter().take_while(|c| {
        used += cells(**c);
        used < width
    });
    left + fitting.count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunScope {
    /// The selection or, without a selection, the statement at the cursor.
    Default,
    Script,
    /// The smallest complete SELECT or WITH subquery at the cursor, without its outer parentheses.
    Subquery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunRefusal {
    CursorOutside,
    NoStatement,
    NoSubquery,
}

/// The result of a format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatOutcome {
    Changed,
    Unchanged,
    /// The text has an open token, so the format has no sure result.
    Refused,
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
    /// The selected row of the whole list: the AI rows first, then the items.
    pub selected: usize,
    pub ai: bool,
}

/// The AI rows above the local items: the request row, then the plain row.
pub const AI_ROWS: usize = 2;

/// Returns how many AI rows a list holds.
pub(crate) fn ai_row_count(ai: bool) -> usize {
    usize::from(ai) * AI_ROWS
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
    suggestions_enabled: bool,
    ai_available: bool,
    ai_requested: Option<self::ai::AiSelection>,
    /// Set when a selection blocks the AI row of an explicit list.
    ai_refused: bool,
    /// The anchor of a running completion, with the time of the request.
    ai_pending: Vec<(Index2, std::time::Instant)>,
    list_area: (Rect, usize),
    literal_text: bool,
    handler: EditorEventHandler,
    dialect: Dialect,
    clipboard: SharedClipboard,
    anchor: Option<Index2>,
    preferred_col: Option<usize>,
    undo_group: Option<UndoGroup>,
    /// The steps of the conventional mode. It is empty in the Vim mode.
    history: UndoHistory,
    /// The redo steps of EdTUI that a new edit did not end.
    redo_depth: usize,
    page_height: usize,
    text_area: Option<Rect>,
    scrollbars: [Option<Scrollbar>; 2],
    scroll_drag: Option<scroll::Drag>,
    scroll_selection_end: Option<Index2>,
    agent_page: std::cell::RefCell<agent::ReadCursor>,
    revision: u64,
    /// The last known text in Vim mode. EdTUI does not report its edits.
    vim_shadow: Option<Lines>,
    snapshot: Option<(u64, Snapshot)>,
    list: Option<SuggestionList>,
    /// The relation whose columns an empty list waits for. The screen shows a note.
    waiting: Option<RelationRef>,
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
    syntax: syntax::Syntax,
    /// The open inline review. The text is read only while it is open.
    review: Option<review::Review>,
    /// The ghost answers that wait for a decision, with their shared view.
    ghosts: Vec<review::Ghost>,
    ghost_view: Option<review::GhostView>,
    ghost_targets: Vec<(review::Control, Rect)>,
    ghost_keys: review::Keys,
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
            suggestions_enabled: true,
            ai_available: false,
            ai_requested: None,
            ai_refused: false,
            ai_pending: Vec::new(),
            list_area: Default::default(),
            literal_text: false,
            handler: EditorEventHandler::vim_mode(),
            dialect,
            clipboard,
            anchor: None,
            preferred_col: None,
            undo_group: None,
            history: UndoHistory::default(),
            redo_depth: 0,
            page_height: 20,
            text_area: None,
            scrollbars: [None, None],
            scroll_drag: None,
            scroll_selection_end: None,
            agent_page: std::cell::RefCell::new(agent::ReadCursor::default()),
            revision: 0,
            vim_shadow,
            snapshot: None,
            list: None,
            waiting: None,
            search: None,
            status: None,
            missing_columns: Vec::new(),
            list_request: None,
            marks: Vec::new(),
            marks_revision: 0,
            plan_revision: 0,
            error_spot: None,
            syntax: syntax::Syntax::default(),
            review: None,
            ghosts: Vec::new(),
            ghost_view: None,
            ghost_targets: Vec::new(),
            ghost_keys: Default::default(),
        }
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Changes the limit of the undo history of the conventional mode.
    pub fn set_undo_limit(&mut self, bytes: usize) {
        self.history.set_limit(bytes);
    }

    /// Changes the SQL rules of the text. The next frame reads the text again.
    pub fn set_dialect(&mut self, dialect: Dialect) {
        self.dialect = dialect;
        self.snapshot = None;
    }

    /// Returns true when typing writes text, so `Ctrl+Space` can open the list.
    pub fn is_inserting(&self) -> bool {
        self.state.mode == EditorMode::Insert
    }

    pub fn keyboard(&self) -> KeyboardMode {
        self.keyboard
    }

    /// Changes the keyboard mode in place. The text, the cursor, the scroll, the
    /// history, and the selection stay. Only the steps that wait for a redo can end.
    pub fn set_keyboard(&mut self, keyboard: KeyboardMode) {
        if self.keyboard == keyboard {
            return;
        }
        self.keyboard = keyboard;
        self.drop_list();
        self.list_request = None;
        self.undo_group = None;
        match keyboard {
            KeyboardMode::Vim => {
                self.history.move_to_edtui(&mut self.state);
                self.anchor = None;
                self.vim_shadow = Some(self.state.lines.clone());
                let selected = self.state.selection.is_some();
                self.state.mode = if selected {
                    EditorMode::Visual
                } else {
                    EditorMode::Normal
                };
                let last = self.state.lines.len_col(self.state.cursor.row).unwrap_or(0);
                self.state.cursor.col = self.state.cursor.col.min(last.saturating_sub(1));
            }
            KeyboardMode::Conventional => {
                self.vim_shadow = None;
                self.state.mode = EditorMode::Insert;
                if let Some(selection) = self.state.selection.take() {
                    let forward = (selection.start.row, selection.start.col)
                        <= (selection.end.row, selection.end.col);
                    let (first, last) = if forward {
                        (selection.start, selection.end)
                    } else {
                        (selection.end, selection.start)
                    };
                    let width = self.state.lines.len_col(last.row).unwrap_or(0);
                    self.anchor = Some(first);
                    self.state.cursor = Index2::new(last.row, (last.col + 1).min(width));
                    self.sync_selection();
                }
            }
        }
    }

    pub fn text(&self) -> String {
        buffer::text_of(&self.state.lines)
    }

    /// Returns true when the editor holds no text.
    pub fn is_blank(&self) -> bool {
        self.state.lines.len() <= 1 && self.state.lines.iter_row().next().is_none_or(Vec::is_empty)
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

    /// Returns the widest line in display cells, including the insertion cursor.
    pub fn horizontal_content_width(&mut self) -> usize {
        let cursor_cell = usize::from(self.state.mode != EditorMode::Normal);
        self.snapshot().max_width + cursor_cell
    }

    /// Returns the horizontal view offset in display cells.
    pub fn horizontal_scroll_position(&mut self) -> usize {
        let (left, _) = self.scroll();
        let row = self.state.cursor.row;
        let snapshot = self.snapshot();
        let start = snapshot.line_start(row);
        let end = snapshot.offset(Index2::new(row, left));
        buffer::display_width(&snapshot.text[start..end])
    }

    pub fn set_scroll(&mut self, column: usize, row: usize) {
        self.state.set_viewport_offset(column, row);
    }

    pub fn set_cursor(&mut self, cursor: Index2) {
        self.scroll_selection_end = None;
        self.state.cursor = cursor;
        self.anchor = None;
        self.break_undo_group();
        self.sync_selection();
    }

    pub fn command_available(&self, action: Action) -> bool {
        match action {
            Action::EditorCopy | Action::EditorCut => self.selection_range().is_some(),
            Action::EditorSelectAll => !self.is_blank(),
            Action::EditorSuggestions => self.is_inserting(),
            Action::EditorUndo => self.revision > 0,
            Action::EditorRedo => self.redo_depth > 0 || self.history.has_redo(),
            _ => true,
        }
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

    /// Gives the message to the application, which ends it.
    pub fn take_status(&mut self) -> Option<&'static str> {
        self.status.take()
    }

    /// Returns the relations that need a catalog read, and it clears the list.
    pub fn take_missing_columns(&mut self) -> Vec<RelationRef> {
        std::mem::take(&mut self.missing_columns)
    }

    fn mark_changed(&mut self) {
        self.revision += 1;
        self.sync_ghosts();
    }

    fn snapshot(&mut self) -> &mut Snapshot {
        let revision = self.revision;
        // The old copy leaves first, so a key keeps one copy at its peak.
        let current = self
            .snapshot
            .take()
            .filter(|(cached, _)| *cached == revision);
        let cached = current.unwrap_or_else(|| {
            (
                revision,
                Snapshot::new(&self.state.lines, self.dialect, self.literal_text),
            )
        });
        &mut self.snapshot.insert(cached).1
    }

    pub fn on_key(&mut self, key: KeyEvent, catalog: Option<&Catalog>) {
        self.restore_scroll_selection();
        self.status = None;
        if self.search.is_some() {
            self.on_search_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(' ') if ctrl => {
                self.open_explicit_suggestions(catalog);
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

    /// Applies an editor command without key simulation.
    pub fn on_command(&mut self, action: Action, catalog: Option<&Catalog>) {
        self.restore_scroll_selection();
        self.status = None;
        self.close_suggestions();
        let mode = self.state.mode;
        match action {
            Action::EditorFind | Action::EditorReplace => {
                self.open_search();
                if action == Action::EditorReplace
                    && let Some(search) = &mut self.search
                {
                    search.field = SearchField::Replace;
                }
            }
            Action::EditorSuggestions => self.open_explicit_suggestions(catalog),
            Action::EditorUndo => self.undo(),
            Action::EditorRedo => self.redo(),
            Action::EditorCopy => self.copy_selection(),
            Action::EditorCut => {
                self.cut_selection();
                self.leave_visual_mode();
            }
            Action::EditorPaste => self.paste_from_clipboard(),
            Action::EditorSelectAll => {
                let start = Index2::new(0, 0);
                let end = buffer::document_end(&self.state.lines);
                if self.keyboard == KeyboardMode::Conventional {
                    self.anchor = Some(start);
                    self.state.cursor = end;
                    self.break_undo_group();
                    self.sync_selection();
                } else {
                    self.state.execute(SwitchMode(EditorMode::Visual));
                    if let Some(selection) = &mut self.state.selection {
                        selection.start = start;
                        selection.end = buffer::previous_position(&self.state.lines, end);
                        self.state.cursor = selection.end;
                    }
                }
            }
            _ => {}
        }
        if self.keyboard == KeyboardMode::Vim
            && matches!(action, Action::EditorUndo | Action::EditorRedo)
        {
            self.state.mode = if mode == EditorMode::Insert {
                mode
            } else {
                EditorMode::Normal
            };
            self.sync_vim_shadow();
        }
    }

    /// Returns the selection as an ordered pair, with the start first.
    pub fn selection_range(&self) -> Option<(Index2, Index2)> {
        match self.keyboard {
            KeyboardMode::Conventional => self.selected_range(),
            KeyboardMode::Vim => self
                .state
                .selection
                .is_some()
                .then(|| self.replace_target()),
        }
    }

    pub fn json_validation(&mut self) -> Option<&crate::grid::json::Validation> {
        self.snapshot().json_validation.as_ref()
    }

    /// Formats the complete cell as one undo step. This action never saves it.
    pub fn format_json(&mut self) -> Result<(), &'static str> {
        if !matches!(
            self.json_validation(),
            Some(crate::grid::json::Validation::Valid)
        ) {
            return Err("Cannot format JSON. Enter a valid object or array within 1 MiB.");
        }
        let source = &self.snapshot().text;
        let formatted = crate::grid::json::pretty(source)
            .ok_or("Formatted JSON exceeds the size limit. Reduce the value before formatting.")?;
        if formatted == *source {
            return Ok(());
        }
        let end = buffer::document_end(&self.state.lines);
        self.replace_ranges(&[(Index2::new(0, 0), end, formatted.as_str())]);
        self.leave_visual_mode();
        Ok(())
    }

    /// Formats the statements of a run, as one undo step. Nothing changes after a refusal.
    pub fn format_sql(&mut self) -> Result<FormatOutcome, RunRefusal> {
        let plan = self.plan_run(RunScope::Default)?;
        let mut changes = Vec::new();
        for (statement, start) in plan.statements.iter().zip(&plan.starts) {
            let Some(formatted) = format_statement(statement, self.dialect) else {
                return Ok(FormatOutcome::Refused);
            };
            if formatted != *statement {
                changes.push((*start, *start + statement.len(), formatted));
            }
        }
        if changes.is_empty() {
            return Ok(FormatOutcome::Unchanged);
        }
        let snapshot = self.snapshot();
        let ranges: Vec<_> = changes
            .iter()
            .map(|(start, end, formatted)| {
                let start = snapshot.position(*start);
                (start, snapshot.position(*end), formatted.as_str())
            })
            .collect();
        self.replace_ranges(&ranges);
        self.leave_visual_mode();
        Ok(FormatOutcome::Changed)
    }

    /// Adds or removes a line comment, as one undo step. It acts on the rows of the
    /// selection, or on the row of the cursor. A selection that ends at the start of a
    /// row leaves that row out.
    pub fn toggle_comment(&mut self) {
        let cursor = self.state.cursor;
        let anchor = self.anchor;
        let (first, last) = match self.selection_range() {
            Some((start, end)) if end.col == 0 && end.row > start.row => (start.row, end.row - 1),
            Some((start, end)) => (start.row, end.row),
            None => (cursor.row, cursor.row),
        };
        let marker = if self.dialect.is_command_line() {
            "#"
        } else {
            "--"
        };
        let lines = &self.state.lines;
        let Some(comment) = toggle_comment(lines, first, last, marker) else {
            return;
        };
        let end = Index2::new(last, buffer::row(lines, last).len());
        self.replace_ranges(&[(Index2::new(first, 0), end, comment.text.as_str())]);
        self.state.cursor = comment.moved(first, cursor);
        self.anchor = anchor.map(|anchor| comment.moved(first, anchor));
        self.sync_selection();
        self.leave_visual_mode();
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

    /// Returns true when EdTUI changed the text. Only the Vim mode has this copy.
    fn sync_vim_shadow(&mut self) -> bool {
        if self
            .vim_shadow
            .as_ref()
            .is_none_or(|shadow| *shadow == self.state.lines)
        {
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
            let last_col = buffer::row(&self.state.lines, cursor.row)
                .len()
                .saturating_sub(1);
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
            let col = cursor
                .col
                .min(buffer::row(&self.state.lines, cursor.row).len());
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

    /// Returns the first column of the next frame. EdTUI moves its view with this rule
    /// during the draw, and the window must be ready before the draw.
    fn next_left(&self, width: usize) -> usize {
        let (left, _) = self.state.viewport_offset();
        let line = buffer::row(&self.state.lines, self.state.cursor.row);
        let last = match self.state.mode {
            EditorMode::Normal => line.len().saturating_sub(1),
            _ => line.len(),
        };
        let cursor = self.state.cursor.col.min(last);
        if cursor < left {
            return cursor;
        }
        if cursor <= last_column(line, left, width) {
            return left;
        }
        let mut used = 0;
        let fitting = (0..=cursor).rev().take_while(|col| {
            used += line.get(*col).map_or(1, |c| cells(*c));
            used < width
        });
        cursor.saturating_sub(fitting.count())
    }

    /// Returns the part of the text that the next frame shows.
    fn next_window(&self, width: usize, height: usize) -> Window {
        let cursor_row = self.state.cursor.row;
        let (_, top) = self.state.viewport_offset();
        let top = if cursor_row < top {
            cursor_row
        } else if cursor_row >= top + height {
            cursor_row + 1 - height
        } else {
            top
        };
        let left = self.next_left(width);
        let ends = (top..=top + height)
            .map(|row| window_end(buffer::row(&self.state.lines, row), left, width))
            .collect();
        Window { top, left, ends }
    }

    pub fn selected_character_count(&self) -> usize {
        let Some((start, end)) = self.selection_range() else {
            return 0;
        };
        if let Some((revision, snapshot)) = &self.snapshot
            && *revision == self.revision
        {
            return snapshot
                .character_offset(end)
                .saturating_sub(snapshot.character_offset(start));
        }
        if start.row == end.row {
            return end.col.saturating_sub(start.col);
        }
        let middle: usize = (start.row..end.row)
            .map(|row| buffer::row(&self.state.lines, row).len() + 1)
            .sum();
        middle + end.col - start.col
    }

    /// Finds the styles of the part of the text that the next frame shows, and returns
    /// the state to draw. Each row stops at the end of the window until that state drops,
    /// so the text is not complete during the draw.
    pub fn prepare_frame(&mut self, width: usize, height: usize, theme: &Theme) -> FrameState<'_> {
        self.page_height = height;
        let window = self.next_window(width, height);
        let dialect = self.dialect;
        let mut styles = Styles::new(&window);
        self.mark_search(&window, &mut styles, theme);
        let spot = self
            .error_spot
            .as_ref()
            .filter(|spot| spot.revision == self.revision)
            .map(|spot| (spot.start, spot.end));
        let warnings: Vec<_> = self
            .syntax_report()
            .into_iter()
            .flat_map(|report| &report.diagnostics)
            .filter_map(|diagnostic| diagnostic.range.clone())
            .collect();
        let snapshot = self.snapshot();
        if let Some((start, end)) = spot {
            let start = snapshot.position(start);
            styles.mark(start, snapshot.position_before(end), theme.error_spot);
        }
        for range in warnings {
            styles.mark(
                snapshot.position(range.start),
                snapshot.position_before(range.end),
                theme.error_spot,
            );
        }
        for (row, end) in window.rows() {
            for (kind, first, last) in snapshot.row_json_tokens(row, window.left, end) {
                styles.mark(
                    Index2::new(row, first),
                    Index2::new(row, last),
                    theme.json_token_style(kind),
                );
            }
            for (token, first, last) in snapshot.row_tokens(row, window.left, end) {
                let word = &snapshot.text[token.start()..token.end()];
                let Some(style) = theme.sql_token_style(token.kind, word, dialect) else {
                    continue;
                };
                styles.mark(Index2::new(row, first), Index2::new(row, last), style);
            }
        }
        let mut rows = Vec::new();
        for (row, end) in window.rows() {
            if let Some(line) = self.state.lines.get_mut(RowIndex::new(row))
                && end < line.len()
            {
                let cut = line[..end].to_vec();
                rows.push((row, std::mem::replace(line, cut)));
            }
        }
        FrameState {
            state: &mut self.state,
            rows,
            styles,
        }
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
