//! The application shell. At this stage it holds one console.

use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    poll, read,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc::Receiver;

use crate::catalog::{Catalog, Columns, Relation, RelationKind, RelationRef, Schema};
use crate::clipboard::{CopyOutcome, SharedClipboard, SystemClipboard};
use crate::connector::sqlite::{SqliteConnector, SqliteTarget, validate_path};
use crate::connector::{Connector, TransactionCommand, TransactionState};
use crate::console::{Console, Focus, GridAction, Phase, Signal};
use crate::editor::{KeyboardMode, RunRefusal, RunScope, SqlEditor};
use crate::exec::{ExecEvent, Manager, Settings};
use crate::grid::Budget;
use crate::sql::Dialect;
use crate::terminal::Session;
use crate::ui::console::BarData;
use crate::ui::theme::Theme;
use crate::ui::wrap::WrappedText;
use crate::ui::{
    ConsoleLayout, column_edge_at, draw_console, draw_dialog, draw_overlay, draw_small_terminal,
    grid_cell_at, tab_at, text,
};

const MIN_WIDTH: u16 = 80;
const MIN_HEIGHT: u16 = 24;
/// The first console. Tabs arrive with the complete interface.
const NO_CONSOLE: u64 = 0;
const EVENTS_PER_FRAME: usize = 64;
const WHEEL_ROWS: isize = 3;
/// The time of one step of the row search, so that a key waits less than a frame.
const SEARCH_SLICE: Duration = Duration::from_millis(8);

pub struct Options {
    pub keyboard: KeyboardMode,
    pub dialect: Dialect,
    pub catalog: Option<Catalog>,
    pub database: Option<PathBuf>,
    /// A ready connector. It has priority over `database`.
    pub connector: Option<Arc<dyn Connector>>,
    /// The name of `connector` in the interface.
    pub label: Option<String>,
    pub settings: Settings,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            keyboard: KeyboardMode::Conventional,
            dialect: Dialect::Postgres,
            catalog: None,
            database: None,
            connector: None,
            label: None,
            settings: Settings::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogAction {
    Dismiss,
    CreateDatabase,
    StopAndClose,
    CancelClose,
    Transaction(TransactionCommand, AfterTransaction),
    CancelModeChange,
    KeepWaiting,
    CloseConnection,
    RunAnyway(RunScope),
}

/// The next step after a commit or a rollback that a dialog asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterTransaction {
    Close,
    UseAutocommit,
}

struct Dialog {
    title: &'static str,
    body: String,
    options: Vec<(&'static str, DialogAction)>,
    focus: usize,
}

/// A drag on the edge of a column in the grid header.
#[derive(Clone, Copy)]
struct ColumnDrag {
    column: usize,
    start: u16,
    width: usize,
}

struct Overlay {
    title: String,
    contents: WrappedText,
    footer: String,
    scroll: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CatalogState {
    NotLoaded,
    Loading,
    Loaded,
    /// The next try waits for an execution, so a key does not repeat a failed read.
    Failed,
}

pub struct App {
    pub console: Console,
    catalog: Option<Catalog>,
    catalog_state: CatalogState,
    requested_columns: Vec<RelationRef>,
    manager: Option<Manager>,
    events: Option<Receiver<ExecEvent>>,
    budget: Arc<Budget>,
    settings: Settings,
    database: Option<PathBuf>,
    label: Option<String>,
    clipboard: SharedClipboard,
    theme: Theme,
    layout: ConsoleLayout,
    dialog: Option<Dialog>,
    overlay: Option<Overlay>,
    status: Option<String>,
    prefix_active: bool,
    column_drag: Option<ColumnDrag>,
    closing: bool,
    pending_transaction: Option<(TransactionCommand, AfterTransaction)>,
    quit: bool,
}

impl App {
    pub fn new(options: Options, clipboard: SharedClipboard, theme: Theme) -> Self {
        let dialect = match (&options.connector, &options.database) {
            (Some(connector), _) => connector.dialect(),
            (None, Some(_)) => Dialect::Sqlite,
            (None, None) => options.dialect,
        };
        let editor = SqlEditor::new("", options.keyboard, dialect, clipboard.clone());
        let budget = Arc::new(Budget::new(options.settings.budget_bytes));
        let mut app = Self {
            console: Console::new(NO_CONSOLE, editor),
            catalog_state: if options.catalog.is_some() {
                CatalogState::Loaded
            } else {
                CatalogState::NotLoaded
            },
            catalog: options.catalog,
            requested_columns: Vec::new(),
            manager: None,
            events: None,
            budget,
            settings: options.settings,
            database: options.database,
            label: options.label,
            clipboard,
            theme,
            layout: ConsoleLayout::default(),
            dialog: None,
            overlay: None,
            status: None,
            prefix_active: false,
            column_drag: None,
            closing: false,
            pending_transaction: None,
            quit: false,
        };
        match options.connector {
            Some(connector) => app.start_manager(connector),
            None => app.prepare_database(),
        }
        app
    }

    fn prepare_database(&mut self) {
        let Some(path) = self.database.clone() else { return };
        if let Err(error) = validate_path(&path) {
            self.status = Some(error.0);
            self.database = None;
            return;
        }
        if path.exists() {
            self.start_sqlite(&path, false);
        } else {
            self.show_dialog(
                text::TITLE_CREATE,
                text::create_database(&path.display().to_string()),
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (text::OPTION_CREATE, DialogAction::CreateDatabase),
                ],
            );
        }
    }

    fn start_sqlite(&mut self, path: &std::path::Path, allow_create: bool) {
        let mut target = SqliteTarget::new(path);
        target.allow_create = allow_create;
        self.start_manager(Arc::new(SqliteConnector::new(target)));
    }

    fn start_manager(&mut self, connector: Arc<dyn Connector>) {
        match Manager::new(connector, self.settings.clone(), self.budget.clone()) {
            Ok((mut manager, events)) => {
                self.console.id = manager.open_console();
                self.manager = Some(manager);
                self.events = Some(events);
            }
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Ends the runtime without a wait for a statement that does not answer.
    pub fn shutdown(&mut self) {
        if let Some(manager) = self.manager.take() {
            manager.shutdown();
        }
    }

    fn show_dialog(
        &mut self,
        title: &'static str,
        body: String,
        options: Vec<(&'static str, DialogAction)>,
    ) {
        self.dialog = Some(Dialog { title, body, options, focus: 0 });
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) if self.dialog.is_none() && self.overlay.is_none() => {
                if !self.console.paste_in_search(&text) {
                    self.console.focus = Focus::Editor;
                    self.console.editor.on_paste(&text);
                }
            }
            Event::Mouse(mouse) if self.dialog.is_none() && self.overlay.is_none() => {
                self.on_mouse(mouse);
            }
            _ => {}
        }
    }

    /// Returns true when the next step must not wait for a key.
    pub fn has_work(&self) -> bool {
        self.console.search_has_work()
    }

    /// Reads waiting execution events and continues the row search.
    /// It returns true when the screen changed.
    pub fn drain_events(&mut self) -> bool {
        let mut changed = self.console.search_has_work();
        if changed {
            self.console.continue_search(Instant::now() + SEARCH_SLICE);
        }
        for _ in 0..EVENTS_PER_FRAME {
            let Some(event) = self.events.as_mut().and_then(|events| events.try_recv().ok()) else {
                break;
            };
            self.on_exec_event(event);
            changed = true;
        }
        changed
    }

    fn on_exec_event(&mut self, event: ExecEvent) {
        match event {
            ExecEvent::CatalogLoaded(Ok(catalog)) => {
                self.catalog = Some(catalog);
                self.catalog_state = CatalogState::Loaded;
                self.requested_columns.clear();
                self.console.editor.refresh_suggestions(self.catalog.as_ref());
                self.request_missing_columns();
            }
            ExecEvent::CatalogLoaded(Err(message)) => {
                self.catalog_state = CatalogState::Failed;
                self.status = Some(message);
            }
            ExecEvent::ColumnsLoaded { relation, columns } => {
                self.requested_columns.retain(|requested| *requested != relation);
                match columns {
                    Ok(columns) => {
                        if let Some(catalog) = &mut self.catalog {
                            catalog.set_columns(&relation, columns);
                        }
                        self.console.editor.refresh_suggestions(self.catalog.as_ref());
                    }
                    Err(message) => self.status = Some(message),
                }
            }
            event => {
                if let Some(signal) = self.console.on_event(event, &self.budget) {
                    self.on_signal(signal);
                }
            }
        }
    }

    fn on_signal(&mut self, signal: Signal) {
        match signal {
            Signal::ExecutionFinished { schema_changed } => {
                if schema_changed {
                    self.reload_catalog();
                }
                if self.closing {
                    self.request_close();
                }
            }
            Signal::ConfirmClose => self.show_dialog(
                text::TITLE_NO_ANSWER,
                text::BODY_NO_ANSWER.to_owned(),
                vec![
                    (text::OPTION_KEEP_WAITING, DialogAction::KeepWaiting),
                    (text::OPTION_CLOSE_CONNECTION, DialogAction::CloseConnection),
                ],
            ),
            Signal::TransactionChanged => {
                if self.console.transaction == TransactionState::None
                    && let Some((_, after)) = self.pending_transaction.take()
                {
                    match after {
                        AfterTransaction::Close => self.request_close(),
                        AfterTransaction::UseAutocommit => self.console.manual = false,
                    }
                }
            }
            Signal::TransactionFailed(message) => self.on_transaction_failed(&message),
        }
    }

    /// The console stays open. A new attempt needs an explicit action on the same session.
    fn on_transaction_failed(&mut self, message: &str) {
        let Some((command, after)) = self.pending_transaction.take() else {
            self.status = Some(text::transaction_failed(message));
            return;
        };
        self.closing = false;
        self.show_dialog(
            text::TITLE_TRANSACTION,
            text::transaction_failed(message),
            vec![
                (text::OPTION_CANCEL, DialogAction::CancelClose),
                (text::OPTION_RETRY, DialogAction::Transaction(command, after)),
            ],
        );
    }

    fn reload_catalog(&mut self) {
        if let Some(manager) = &self.manager {
            manager.load_catalog();
            self.catalog_state = CatalogState::Loading;
        }
    }

    /// Reads the catalog at its first use, so the start opens no connection.
    fn ensure_catalog(&mut self) {
        if self.catalog_state == CatalogState::NotLoaded && self.manager.is_some() {
            self.reload_catalog();
        }
    }

    fn request_missing_columns(&mut self) {
        let Some(manager) = &self.manager else { return };
        for relation in self.console.editor.take_missing_columns() {
            if !self.requested_columns.contains(&relation) {
                manager.load_columns(relation.clone());
                self.requested_columns.push(relation);
            }
        }
    }

    /// Applies the key priority of `docs/KEYMAP.md`.
    fn on_key(&mut self, key: KeyEvent) {
        self.status = None;
        if self.dialog.is_some() {
            self.on_dialog_key(key);
            return;
        }
        if self.overlay.is_some() {
            self.on_overlay_key(key);
            return;
        }
        if self.console.focus == Focus::Editor && self.console.editor.on_list_key(key) {
            return;
        }
        if self.prefix_active {
            self.prefix_active = false;
            self.on_prefix_key(key);
            return;
        }
        if key.code == KeyCode::Char('w') && key.modifiers == KeyModifiers::CONTROL {
            self.prefix_active = true;
            return;
        }
        if self.on_global_key(key) {
            return;
        }
        match self.console.focus {
            Focus::Editor => {
                self.ensure_catalog();
                self.console.editor.on_key(key, self.catalog.as_ref());
                self.request_missing_columns();
            }
            Focus::Results => match self.console.on_results_key(key) {
                GridAction::None => {}
                GridAction::Copy(tsv) => self.copy(&tsv),
                GridAction::OpenValue { title, text, footer } => {
                    let contents = WrappedText::new(text);
                    self.overlay = Some(Overlay { title, contents, footer, scroll: 0 });
                }
                GridAction::CloseResult => {
                    if !self.console.close_result(&self.budget) {
                        self.status = Some(text::RESULT_READING.to_owned());
                    }
                }
            },
        }
    }

    fn on_global_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::F(5) => self.run(if shift { RunScope::Script } else { RunScope::Default }),
            KeyCode::Enter if ctrl => self.run(RunScope::Default),
            KeyCode::Enter if shift => self.run(RunScope::Script),
            KeyCode::F(1) => self.open_keys(),
            _ => return false,
        }
        true
    }

    fn on_prefix_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') => self.request_close(),
            KeyCode::Char('e') => self.run(RunScope::Default),
            KeyCode::Char('s') => self.run(RunScope::Script),
            KeyCode::Char('x') => self.stop(),
            KeyCode::Char('m') => self.toggle_mode(),
            KeyCode::Char('C') => self.transaction(TransactionCommand::Commit),
            KeyCode::Char('R') => self.transaction(TransactionCommand::Rollback),
            KeyCode::Char('j') | KeyCode::Down => {
                self.console.editor.close_suggestions();
                self.console.focus = Focus::Results;
            }
            KeyCode::Char('k') | KeyCode::Up => self.console.focus = Focus::Editor,
            KeyCode::Char('?') => self.open_keys(),
            _ => {}
        }
    }

    fn open_keys(&mut self) {
        self.overlay = Some(Overlay {
            title: format!(" {} ", text::TITLE_KEYS),
            contents: WrappedText::new(text::KEYS_SCREEN.to_owned()),
            footer: String::new(),
            scroll: 0,
        });
    }

    fn run(&mut self, scope: RunScope) {
        if self.manager.is_none() {
            self.status = Some(text::NO_CONNECTION.to_owned());
            return;
        }
        if self.console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if self.console.transaction == TransactionState::Unknown {
            self.show_dialog(
                text::TITLE_UNKNOWN,
                text::BODY_UNKNOWN.to_owned(),
                vec![
                    (text::OPTION_CANCEL, DialogAction::Dismiss),
                    (text::OPTION_CONTINUE, DialogAction::RunAnyway(scope)),
                ],
            );
            return;
        }
        self.start_run(scope);
    }

    fn start_run(&mut self, scope: RunScope) {
        let Some(manager) = &self.manager else { return };
        match self.console.editor.plan_run(scope) {
            Ok(plan) => {
                let manual = self.console.manual;
                let execution =
                    manager.run(self.console.id, plan.statements.clone(), manual);
                self.console.begin_execution(execution, &plan, &self.budget);
            }
            Err(RunRefusal::CursorOutside) => {
                self.status = Some(text::CURSOR_OUTSIDE.to_owned());
            }
            Err(RunRefusal::NoStatement) => self.status = Some(text::NO_STATEMENT.to_owned()),
        }
    }

    fn stop(&mut self) {
        if !self.console.is_busy() {
            return;
        }
        if let Some(manager) = &self.manager {
            manager.stop(self.console.id);
            self.console.phase = Phase::Stopping;
        }
    }

    fn transaction(&mut self, command: TransactionCommand) {
        if self.console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if !self.console.in_transaction() {
            self.status = Some(text::NO_TRANSACTION.to_owned());
            return;
        }
        if let Some(manager) = &self.manager {
            manager.transaction(self.console.id, command);
        }
    }

    fn toggle_mode(&mut self) {
        if self.console.is_busy() {
            self.status = Some(text::EXECUTION_ACTIVE.to_owned());
            return;
        }
        if !self.console.manual {
            self.console.manual = true;
            return;
        }
        if !self.console.in_transaction() {
            self.console.manual = false;
            return;
        }
        let after = AfterTransaction::UseAutocommit;
        self.show_dialog(
            text::TITLE_TRANSACTION,
            text::BODY_TRANSACTION_MODE.to_owned(),
            vec![
                (text::OPTION_CANCEL, DialogAction::CancelModeChange),
                (text::OPTION_ROLLBACK_AUTO, DialogAction::Transaction(TransactionCommand::Rollback, after)),
                (text::OPTION_COMMIT_AUTO, DialogAction::Transaction(TransactionCommand::Commit, after)),
            ],
        );
    }

    /// Asks each question that the close needs. It runs again after each answer.
    fn request_close(&mut self) {
        if self.console.is_busy() {
            if self.closing {
                return;
            }
            self.show_dialog(
                text::TITLE_RUNNING,
                text::BODY_RUNNING.to_owned(),
                vec![
                    (text::OPTION_CANCEL, DialogAction::CancelClose),
                    (text::OPTION_STOP_CLOSE, DialogAction::StopAndClose),
                ],
            );
            return;
        }
        if self.console.in_transaction() {
            let after = AfterTransaction::Close;
            self.closing = false;
            self.show_dialog(
                text::TITLE_TRANSACTION,
                text::BODY_TRANSACTION_CLOSE.to_owned(),
                vec![
                    (text::OPTION_CANCEL, DialogAction::CancelClose),
                    (text::OPTION_ROLLBACK_CLOSE, DialogAction::Transaction(TransactionCommand::Rollback, after)),
                    (text::OPTION_COMMIT_CLOSE, DialogAction::Transaction(TransactionCommand::Commit, after)),
                ],
            );
            return;
        }
        self.quit = true;
    }

    fn on_dialog_key(&mut self, key: KeyEvent) {
        let vim = self.console.editor.keyboard() == KeyboardMode::Vim;
        let Some(dialog) = &mut self.dialog else { return };
        let count = dialog.options.len();
        match key.code {
            KeyCode::Tab | KeyCode::Right => dialog.focus = (dialog.focus + 1) % count,
            KeyCode::BackTab | KeyCode::Left => dialog.focus = (dialog.focus + count - 1) % count,
            KeyCode::Char('l') if vim => dialog.focus = (dialog.focus + 1) % count,
            KeyCode::Char('h') if vim => dialog.focus = (dialog.focus + count - 1) % count,
            KeyCode::Enter => {
                let action = dialog.options[dialog.focus].1;
                self.dialog = None;
                self.on_dialog_action(action);
            }
            KeyCode::Esc => {
                // The first option never changes the database or the disk.
                let action = dialog.options[0].1;
                self.dialog = None;
                self.on_dialog_action(action);
            }
            _ => {}
        }
    }

    fn on_dialog_action(&mut self, action: DialogAction) {
        match action {
            DialogAction::Dismiss | DialogAction::CancelModeChange => {}
            DialogAction::CancelClose => self.closing = false,
            DialogAction::CreateDatabase => {
                if let Some(path) = self.database.clone() {
                    self.start_sqlite(&path, true);
                }
            }
            DialogAction::StopAndClose => {
                self.closing = true;
                self.stop();
            }
            DialogAction::Transaction(command, after) => {
                self.pending_transaction = Some((command, after));
                if after == AfterTransaction::Close {
                    self.closing = true;
                }
                if let Some(manager) = &self.manager {
                    manager.transaction(self.console.id, command);
                }
            }
            DialogAction::KeepWaiting => self.confirm_close(false),
            DialogAction::CloseConnection => self.confirm_close(true),
            DialogAction::RunAnyway(scope) => {
                self.console.transaction = TransactionState::None;
                self.start_run(scope);
            }
        }
    }

    fn confirm_close(&self, close: bool) {
        if let Some(manager) = &self.manager {
            manager.confirm_close(self.console.id, close);
        }
    }

    fn on_overlay_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(overlay) = &mut self.overlay else { return };
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.overlay = None,
            KeyCode::Up | KeyCode::Char('k') => overlay.scroll = overlay.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => overlay.scroll = overlay.scroll.saturating_add(1),
            KeyCode::Char('c') if ctrl => {
                let contents = overlay.contents.text().to_owned();
                self.copy(&contents);
            }
            _ => {}
        }
    }

    fn copy(&mut self, text: &str) {
        let outcome = self.clipboard.borrow_mut().copy(text);
        self.status = Some(
            match outcome {
                CopyOutcome::Copied => text::COPIED,
                CopyOutcome::SentToTerminal => text::COPY_SENT_TO_TERMINAL,
                CopyOutcome::Failed => text::COPY_FAILED,
            }
            .to_owned(),
        );
    }

    fn on_mouse(&mut self, mouse: MouseEvent) {
        let position = ratatui::layout::Position::new(mouse.column, mouse.row);
        let in_results = self.layout.results.contains(position);
        let in_editor = self.layout.editor.contains(position);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if self.layout.tabs.contains(position) => {
                if let Some(tab) = tab_at(&self.console, self.layout.tabs, mouse.column) {
                    self.console.active = tab;
                }
            }
            MouseEventKind::Down(MouseButton::Left) if in_results => {
                self.console.focus = Focus::Results;
                if !self.start_column_drag(mouse) {
                    self.point_at_cell(mouse, false);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.column_drag.is_some() => {
                self.drag_column(mouse);
            }
            MouseEventKind::Up(_) if self.column_drag.is_some() => self.column_drag = None,
            MouseEventKind::Drag(MouseButton::Left) if self.console.focus == Focus::Results => {
                self.point_at_cell(mouse, true);
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if in_results => {
                let rows = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                    -WHEEL_ROWS
                } else {
                    WHEEL_ROWS
                };
                if let Some(result) = self.console.results.get_mut(self.console.active) {
                    result.view.move_by(rows, 0, &result.table, false);
                }
            }
            MouseEventKind::Down(_) if in_editor => {
                self.console.focus = Focus::Editor;
                self.console.editor.on_mouse(mouse);
            }
            _ if in_editor && self.console.focus == Focus::Editor => {
                self.console.editor.on_mouse(mouse);
            }
            _ => {}
        }
    }

    fn start_column_drag(&mut self, mouse: MouseEvent) -> bool {
        let Some(result) = self.console.results.get(self.console.active) else { return false };
        let area = self.layout.results;
        self.column_drag = column_edge_at(result, area, mouse.column, mouse.row).map(|column| {
            ColumnDrag { column, start: mouse.column, width: result.table.width(column) }
        });
        self.column_drag.is_some()
    }

    fn drag_column(&mut self, mouse: MouseEvent) {
        let Some(drag) = self.column_drag else { return };
        let Some(result) = self.console.results.get_mut(self.console.active) else { return };
        let change = isize::from(mouse.column as i16 - drag.start as i16);
        result.table.set_width(drag.column, drag.width.saturating_add_signed(change));
    }

    fn point_at_cell(&mut self, mouse: MouseEvent, extend: bool) {
        let area = self.layout.results;
        let Some(result) = self.console.results.get_mut(self.console.active) else { return };
        if let Some((row, col)) = grid_cell_at(result, area, mouse.column, mouse.row) {
            result.view.move_to(row, col, &result.table, extend);
        }
    }

    fn hint(&self) -> &'static str {
        if self.dialog.is_some() {
            text::HINT_DIALOG
        } else if self.overlay.is_some() {
            text::HINT_OVERLAY
        } else if self.prefix_active {
            text::HINT_PREFIX
        } else if self.console.focus == Focus::Results && self.console.search_is_open() {
            text::HINT_GRID_SEARCH
        } else if self.console.focus == Focus::Results {
            text::HINT_GRID
        } else if self.console.editor.suggestions().is_some() {
            text::HINT_LIST
        } else if self.console.editor.search().is_some() {
            text::HINT_SEARCH
        } else if self.console.is_busy() {
            text::HINT_RUNNING
        } else {
            text::HINT_EDITOR
        }
    }

    fn connection_label(&self) -> String {
        if let Some(label) = &self.label {
            return label.clone();
        }
        match &self.database {
            Some(path) => {
                let name = path.file_name().unwrap_or(path.as_os_str());
                name.to_string_lossy().into_owned()
            }
            None if self.manager.is_some() => text::DATABASE_LABEL.to_owned(),
            None => text::NO_DATABASE_LABEL.to_owned(),
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            draw_small_terminal(frame, area, (MIN_WIDTH, MIN_HEIGHT));
            return;
        }
        let [console, hints] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
        let connection = self.connection_label();
        let bar = BarData {
            connection: &connection,
            pool: self.manager.as_ref().map(|manager| manager.pool().usage()),
            data: (self.budget.used(), self.budget.limit()),
        };
        self.layout = draw_console(frame, console, &mut self.console, &bar, &self.theme);
        self.draw_hint(frame, hints);
        self.draw_layers(frame);
    }

    fn draw_hint(&self, frame: &mut Frame, area: Rect) {
        let status = self.status.as_deref().or(self.console.editor.status());
        let line = match status {
            Some(status) => Span::styled(status, self.theme.status),
            None => Span::styled(self.hint(), self.theme.hint),
        };
        frame.render_widget(Paragraph::new(Line::from(line)), area);
    }

    fn draw_layers(&mut self, frame: &mut Frame) {
        if let Some(overlay) = &mut self.overlay {
            let Overlay { title, contents, footer, scroll } = overlay;
            *scroll = draw_overlay(frame, title, contents, footer, *scroll, &self.theme);
        }
        if let Some(dialog) = &self.dialog {
            let labels: Vec<&str> = dialog.options.iter().map(|(label, _)| *label).collect();
            draw_dialog(frame, dialog.title, &dialog.body, &labels, dialog.focus, &self.theme);
        }
    }
}

pub fn run(options: Options) -> io::Result<()> {
    let clipboard: SharedClipboard = Rc::new(RefCell::new(SystemClipboard::new()));
    let mut session = Session::start()?;
    let mut app = App::new(options, clipboard, Theme::from_env());
    let mut dirty = true;
    loop {
        if dirty {
            session.screen.draw(|frame| app.draw(frame))?;
            dirty = false;
        }
        if app.should_quit() {
            app.shutdown();
            return Ok(());
        }
        let wait = if app.has_work() { Duration::ZERO } else { Duration::from_millis(20) };
        if poll(wait)? {
            app.on_event(read()?);
            while poll(Duration::ZERO)? {
                app.on_event(read()?);
            }
            dirty = true;
        }
        dirty |= app.drain_events();
    }
}

/// A fixed catalog for a session without a database.
pub fn sample_catalog() -> Catalog {
    let relation = |name: &str, kind, columns: &[&str]| Relation {
        name: name.to_owned(),
        kind,
        columns: Columns::Loaded(columns.iter().map(|column| (*column).to_owned()).collect()),
    };
    Catalog {
        schemas: vec![
            Schema {
                name: "public".to_owned(),
                relations: vec![
                    relation("users", RelationKind::Table, &["id", "name", "email", "created_at"]),
                    relation("orders", RelationKind::Table, &["id", "user_id", "total", "status"]),
                    relation("active_users", RelationKind::View, &["id", "name"]),
                    relation("Order Items", RelationKind::Table, &["order_id", "sku", "quantity"]),
                ],
            },
            Schema {
                name: "sales".to_owned(),
                relations: vec![relation("invoices", RelationKind::Table, &["number", "due_date"])],
            },
        ],
        current_schema: Some("public".to_owned()),
    }
}
