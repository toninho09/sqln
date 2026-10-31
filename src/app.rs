//! The application shell: the consoles, the connections, and the screen.

mod close;
mod connections;
mod keys;
mod mouse;
mod tables;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyEventKind, poll, read};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc::Receiver;

use self::mouse::Drag;
use crate::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use crate::clipboard::{SharedClipboard, SystemClipboard};
use crate::connection::{Connection, Definition};
use crate::connector::{Connector, Namespace, TransactionCommand};
use crate::console::{AfterTransaction, Console, Focus};
use crate::editor::{KeyboardMode, RunScope, SqlEditor};
use crate::exec::{ConnectionId, ConsoleId, ExecEvent, Manager, Settings};
use crate::form::ConnectionForm;
use crate::grid::Budget;
use crate::input::{Chord, Keymap};
use crate::navigator::Navigator;
use crate::sql::Dialect;
use crate::table::{TableFocus, TableTab};
use crate::terminal::Session;
use crate::ui::console::BarData;
use crate::ui::form::{FormLayout, draw_form};
use crate::ui::navigator::draw_navigator;
use crate::ui::theme::Theme;
use crate::ui::wrap::WrappedText;
use crate::ui::{
    ConsoleLayout, TableBar, TableLayout, draw_console, draw_dialog, draw_main_tabs, draw_overlay,
    draw_picker, draw_small_terminal, draw_table, text,
};

const MIN_WIDTH: u16 = 80;
const MIN_HEIGHT: u16 = 24;
/// A console without a connection takes its identity from the end of the range,
/// and the manager gives identities from the start.
const FIRST_UNBOUND_CONSOLE: ConsoleId = u64::MAX;
const EVENTS_PER_FRAME: usize = 64;
/// The time of one step of the row search, so that a key waits less than a frame.
const SEARCH_SLICE: Duration = Duration::from_millis(8);
const DEFAULT_EDITOR_RATIO: u16 = 50;
const MIN_EDITOR_RATIO: u16 = 10;
const MAX_EDITOR_RATIO: u16 = 90;

pub struct Options {
    pub keyboard: KeyboardMode,
    /// The key map. Without a value, the defaults of `keyboard` apply.
    pub keymap: Option<Keymap>,
    pub dialect: Dialect,
    pub catalog: Option<Catalog>,
    pub database: Option<PathBuf>,
    /// The connections of the command line. The first console uses the first one.
    pub connections: Vec<Definition>,
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
            keymap: None,
            dialect: Dialect::Postgres,
            catalog: None,
            database: None,
            connections: Vec::new(),
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
    CreateFormFile,
    StopAndClose(ConsoleId),
    CancelClose(ConsoleId),
    Transaction(ConsoleId, TransactionCommand, AfterTransaction),
    KeepWaiting(ConsoleId),
    CloseConnection(ConsoleId),
    RunAnyway(ConsoleId, RunScope),
    RemoveConnection(ConnectionId),
}

struct Dialog {
    title: String,
    body: String,
    options: Vec<(&'static str, DialogAction)>,
    focus: usize,
}

struct Overlay {
    title: String,
    contents: WrappedText,
    footer: String,
    scroll: usize,
}

/// The list that selects the schema or the database of a console.
struct Picker {
    console: ConsoleId,
    connection: ConnectionId,
    title: &'static str,
    /// The schema names. The first row of the list is the default of the connection.
    items: Vec<String>,
    cursor: usize,
    loading: bool,
}

/// The screen areas of the last frame. The mouse uses them.
#[derive(Clone, Debug, Default)]
struct ScreenLayout {
    navigator: Rect,
    divider: Option<u16>,
    main_tabs: Vec<(u16, u16, usize)>,
    body: Rect,
    console: ConsoleLayout,
    table: TableLayout,
    form: FormLayout,
    /// The options of the open dialog, with the row of those options.
    dialog: (Vec<(u16, u16, usize)>, u16),
    /// The area of the open list, with the index of its first row.
    picker: (Rect, usize),
}

pub struct App {
    consoles: Vec<Console>,
    /// The console that the console commands use. A table tab can be on screen.
    active: usize,
    /// The table tabs. They follow the consoles in the tab row.
    tables: Vec<TableTab>,
    /// The table tab on screen, or `None` when the active console is on screen.
    table_active: Option<usize>,
    connections: Vec<Connection>,
    navigator: Navigator,
    navigator_focused: bool,
    /// The catalog of a console without a connection.
    fallback_catalog: Option<Catalog>,
    manager: Option<Manager>,
    events: Option<Receiver<ExecEvent>>,
    budget: Arc<Budget>,
    settings: Settings,
    keyboard: KeyboardMode,
    keymap: Keymap,
    database: Option<PathBuf>,
    clipboard: SharedClipboard,
    theme: Theme,
    layout: ScreenLayout,
    editor_ratio: u16,
    /// The first dialog is open. Another console can add a question at any time.
    dialogs: VecDeque<Dialog>,
    picker: Option<Picker>,
    form: Option<ConnectionForm>,
    overlay: Option<Overlay>,
    status: Option<String>,
    /// The first key of a sequence such as `Ctrl+W e`.
    pending_key: Option<Chord>,
    drag: Option<Drag>,
    /// The time and the place of the last click in the navigator.
    last_click: Option<(Instant, u16, u16)>,
    next_console_number: usize,
    next_unbound_console: ConsoleId,
    next_test: u64,
    exiting: bool,
    quit: bool,
}

impl App {
    pub fn new(options: Options, clipboard: SharedClipboard, theme: Theme) -> Self {
        let dialect = match (&options.connector, options.connections.first(), &options.database) {
            (Some(connector), ..) => connector.dialect(),
            (None, _, Some(_)) => Dialect::Sqlite,
            (None, Some(definition), None) => definition.connector().dialect(),
            (None, None, None) => options.dialect,
        };
        let budget = Arc::new(Budget::new(options.settings.budget_bytes));
        let keymap = options.keymap.unwrap_or_else(|| Keymap::new(options.keyboard));
        let mut app = Self {
            consoles: Vec::new(),
            active: 0,
            tables: Vec::new(),
            table_active: None,
            connections: Vec::new(),
            navigator: Navigator::default(),
            navigator_focused: false,
            fallback_catalog: options.catalog,
            manager: None,
            events: None,
            budget,
            settings: options.settings,
            keyboard: options.keyboard,
            keymap,
            database: options.database,
            clipboard,
            theme,
            layout: ScreenLayout::default(),
            editor_ratio: DEFAULT_EDITOR_RATIO,
            dialogs: VecDeque::new(),
            picker: None,
            form: None,
            overlay: None,
            status: None,
            pending_key: None,
            drag: None,
            last_click: None,
            next_console_number: 1,
            next_unbound_console: FIRST_UNBOUND_CONSOLE,
            next_test: 1,
            exiting: false,
            quit: false,
        };
        let console = app.unbound_console(dialect);
        app.consoles.push(console);
        if let Some(connector) = options.connector {
            let name = options.label.unwrap_or_else(|| text::DATABASE_LABEL.to_owned());
            app.add_connection(name, None, connector);
        } else {
            app.prepare_database();
        }
        for definition in options.connections {
            app.add_definition(definition);
        }
        if let Some(connection) = app.connections.first().map(|connection| connection.id) {
            app.bind_console(0, connection);
        }
        app
    }

    fn unbound_console(&mut self, dialect: Dialect) -> Console {
        let editor = SqlEditor::new("", self.keyboard, dialect, self.clipboard.clone());
        let name = text::console_name(self.next_console_number);
        self.next_console_number += 1;
        let id = self.next_unbound_console;
        self.next_unbound_console -= 1;
        Console::new(id, name, editor)
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

    /// Returns the console of the active tab.
    pub fn console(&self) -> &Console {
        &self.consoles[self.active]
    }

    pub fn console_mut(&mut self) -> &mut Console {
        &mut self.consoles[self.active]
    }

    pub fn consoles(&self) -> &[Console] {
        &self.consoles
    }

    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    pub fn navigator(&self) -> &Navigator {
        &self.navigator
    }

    pub fn navigator_focused(&self) -> bool {
        self.navigator_focused
    }

    pub fn form(&self) -> Option<&ConnectionForm> {
        self.form.as_ref()
    }

    fn console_index(&self, id: ConsoleId) -> Option<usize> {
        self.consoles.iter().position(|console| console.id == id)
    }

    fn console_by_id(&mut self, id: ConsoleId) -> Option<&mut Console> {
        self.consoles.iter_mut().find(|console| console.id == id)
    }

    fn connection(&self, id: ConnectionId) -> Option<&Connection> {
        self.connections.iter().find(|connection| connection.id == id)
    }

    fn connection_mut(&mut self, id: ConnectionId) -> Option<&mut Connection> {
        self.connections.iter_mut().find(|connection| connection.id == id)
    }

    fn active_connection(&self) -> Option<&Connection> {
        self.console().connection.and_then(|id| self.connection(id))
    }

    /// Adds a question. It opens now, or after the questions before it.
    fn show_dialog(
        &mut self,
        title: impl Into<String>,
        body: String,
        options: Vec<(&'static str, DialogAction)>,
    ) {
        self.dialogs.push_back(Dialog { title: title.into(), body, options, focus: 0 });
    }

    pub fn on_event(&mut self, event: Event) {
        let blocked = !self.dialogs.is_empty() || self.overlay.is_some() || self.picker.is_some();
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) if !blocked => self.on_paste(&text),
            Event::Mouse(mouse) if !self.dialogs.is_empty() => self.on_dialog_mouse(mouse),
            Event::Mouse(mouse) if self.picker.is_some() => self.on_picker_mouse(mouse),
            Event::Mouse(mouse) if !blocked => self.on_mouse(mouse),
            _ => {}
        }
    }

    fn on_paste(&mut self, text: &str) {
        if let Some(form) = &mut self.form {
            form.on_paste(text);
            return;
        }
        if self.navigator_focused {
            return;
        }
        if let Some(tab) = self.table_mut() {
            if tab.focus == TableFocus::Grid {
                tab.result.paste_in_search(text);
            } else {
                tab.paste_in_field(text);
            }
            return;
        }
        let console = self.console_mut();
        if !console.paste_in_search(text) {
            console.focus = Focus::Editor;
            console.editor.on_paste(text);
        }
    }

    /// Returns true when the next step must not wait for a key.
    pub fn has_work(&self) -> bool {
        match self.table() {
            Some(tab) => tab.result.search_has_work(),
            None => self.console().search_has_work(),
        }
    }

    /// Reads waiting execution events and continues the row search.
    /// It returns true when the screen changed.
    pub fn drain_events(&mut self) -> bool {
        let changed = self.has_work();
        if changed {
            let deadline = Instant::now() + SEARCH_SLICE;
            match self.table_active {
                Some(index) => self.tables[index].result.continue_search(deadline),
                None => self.console_mut().continue_search(deadline),
            }
        }
        let mut changed = changed;
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
            ExecEvent::CatalogLoaded { connection, catalog } => {
                self.on_catalog_loaded(connection, catalog);
            }
            ExecEvent::ColumnsLoaded { connection, relation, columns } => {
                self.on_columns_loaded(connection, &relation, columns);
            }
            ExecEvent::ConnectionTested { test, result } => self.on_connection_tested(test, result),
            ExecEvent::Disconnected { connection } => self.on_disconnected(connection),
            event => self.on_console_event(event),
        }
    }

    fn on_console_event(&mut self, event: ExecEvent) {
        let Some(id) = event.console() else { return };
        let budget = self.budget.clone();
        if let Some(tab) = self.tables.iter_mut().find(|tab| tab.id == id) {
            tab.on_event(event, &budget);
            return;
        }
        match self.console_by_id(id) {
            Some(console) => {
                if let Some(signal) = console.on_event(event, &budget) {
                    self.on_signal(id, signal);
                }
            }
            // A closed console has no grid, so its data leaves the budget.
            None => {
                if let ExecEvent::Rows { bytes, .. } = event {
                    budget.release(bytes);
                }
            }
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            draw_small_terminal(frame, area, (MIN_WIDTH, MIN_HEIGHT));
            return;
        }
        let [body, hints] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
        self.layout.body = body;
        let main = self.draw_side(frame, body);
        let [tabs, console] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(main);
        let labels = self.tab_labels();
        self.layout.main_tabs = draw_main_tabs(frame, tabs, labels, self.active_tab(), &self.theme);
        if self.table_active.is_some() {
            self.draw_active_table(frame, console);
        } else {
            self.draw_active_console(frame, console);
        }
        self.draw_hint(frame, hints);
        self.draw_layers(frame);
    }

    /// Draws the navigator and its divider. It returns the area for the console.
    fn draw_side(&mut self, frame: &mut Frame, body: Rect) -> Rect {
        if self.navigator.collapsed {
            self.layout.navigator = Rect::default();
            self.layout.divider = None;
            return body;
        }
        let width = self.navigator.width.min(body.width.saturating_sub(mouse::MIN_MAIN_WIDTH));
        let [side, divider, main] = Layout::horizontal([
            Constraint::Length(width),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(body);
        let focused = self.navigator_focused;
        let connected: Vec<bool> = self
            .connections
            .iter()
            .map(|connection| {
                self.manager.as_ref().is_some_and(|manager| manager.is_connected(connection.id))
            })
            .collect();
        draw_navigator(frame, side, &mut self.navigator, &self.connections, &connected, focused, &self.theme);
        let line = vec![Line::raw("│"); usize::from(divider.height)];
        frame.render_widget(Paragraph::new(line).style(self.theme.divider), divider);
        self.layout.navigator = side;
        self.layout.divider = Some(divider.x);
        main
    }

    fn draw_active_console(&mut self, frame: &mut Frame, area: Rect) {
        let connection = self.active_connection();
        let name = connection.map_or(text::NO_DATABASE_LABEL, |connection| &connection.name).to_owned();
        let namespace = connection.map_or(Namespace::None, |item| item.capabilities.namespace);
        let transactions = connection.is_none_or(|item| item.capabilities.transactions);
        let default_schema = connection.and_then(|item| item.default_schema.clone());
        let pool = self.console().connection.zip(self.manager.as_ref()).map(|(id, manager)| manager.usage(id));
        let bar = BarData {
            connection: &name,
            pool,
            data: (self.budget.used(), self.budget.limit()),
            namespace,
            default_schema: default_schema.as_deref(),
            transactions,
        };
        let ratio = self.editor_ratio;
        let focused = !self.navigator_focused;
        let console = &mut self.consoles[self.active];
        self.layout.console = draw_console(frame, area, console, &bar, ratio, focused, &self.theme);
    }

    fn draw_active_table(&mut self, frame: &mut Frame, area: Rect) {
        let Some(index) = self.table_active else { return };
        let connection = self.tables[index].connection;
        let name = self.connection(connection).map_or_else(String::new, |item| item.name.clone());
        let pool = self.manager.as_ref().map(|manager| manager.usage(connection));
        let bar = TableBar {
            connection: &name,
            pool,
            data: (self.budget.used(), self.budget.limit()),
            blocked: self.tables[index].is_blocked(&self.budget),
        };
        let focused = !self.navigator_focused;
        self.layout.table = draw_table(frame, area, &mut self.tables[index], &bar, focused, &self.theme);
    }

    fn draw_hint(&self, frame: &mut Frame, area: Rect) {
        let editor_status = if self.table_active.is_some() { None } else { self.console().editor.status() };
        let status = self.status.as_deref().or(editor_status);
        let line = match status {
            Some(status) => Span::styled(status, self.theme.status),
            None => Span::styled(self.hint(), self.theme.hint),
        };
        frame.render_widget(Paragraph::new(Line::from(line)), area);
    }

    fn draw_layers(&mut self, frame: &mut Frame) {
        if let Some(form) = &self.form {
            self.layout.form = draw_form(frame, form, &self.theme);
        }
        if let Some(picker) = &self.picker {
            let default = text::SCHEMA_DEFAULT;
            let items: Vec<&str> =
                std::iter::once(default).chain(picker.items.iter().map(String::as_str)).collect();
            self.layout.picker =
                draw_picker(frame, picker.title, &items, picker.cursor, picker.loading, &self.theme);
        }
        if let Some(overlay) = &mut self.overlay {
            let Overlay { title, contents, footer, scroll } = overlay;
            *scroll = draw_overlay(frame, title, contents, footer, *scroll, &self.theme);
        }
        if let Some(dialog) = self.dialogs.front() {
            let labels: Vec<&str> = dialog.options.iter().map(|(label, _)| *label).collect();
            self.layout.dialog =
                draw_dialog(frame, &dialog.title, &dialog.body, &labels, dialog.focus, &self.theme);
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
