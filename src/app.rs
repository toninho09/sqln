//! The application shell: the consoles, the connections, and the screen.

mod auth;
mod close;
mod conflicts;
mod connections;
mod keys;
mod mouse;
mod persist;
mod scripts;
mod session;
mod tables;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyEventKind, MouseEventKind, poll, read};
use edtui::EditorMode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use tokio::sync::mpsc::Receiver;

use self::mouse::Drag;
use crate::catalog::Catalog;
use crate::clipboard::{SharedClipboard, SystemClipboard};
use crate::connection::{Connection, Definition};
use crate::connector::{Connector, Namespace, TransactionCommand};
use crate::console::{AfterTransaction, Console, Focus};
use crate::creds::{Identity, PlainFile};
use crate::editor::{KeyboardMode, RunScope, SqlEditor};
use crate::exec::{ConnectionId, ConsoleId, ExecEvent, Manager, Settings};
use crate::form::ConnectionForm;
use crate::grid::Budget;
use crate::input::{Chord, Keymap, SuggestionKeys};
use crate::navigator::Navigator;
use crate::palette::Palette;
use crate::settings::SettingsForm;
use crate::sql::Dialect;
use crate::store::config::{IconName, ThemeName};
use crate::store::scripts::ScriptEntry;
use crate::store::{Scope, Store};
use crate::table::{TableFocus, TableTab};
use crate::terminal::Session;
use crate::ui::console::BarData;
use crate::ui::form::{FormLayout, draw_form};
use crate::ui::name::{NamePrompt, draw_name};
use crate::ui::navigator::draw_navigator;
use crate::ui::palette::draw_palette;
use crate::ui::settings::draw_settings;
use crate::ui::statusline::{StatusData, StatusPanel, draw_statusline, hint_line, run_time};
use crate::ui::theme::{ColorEnv, Theme};
use crate::ui::which_key::draw_which_key;
use crate::ui::wrap::WrappedText;
use crate::ui::{
    ConflictLayout, ConflictRowView, ConsoleLayout, MainTab, TableBar, TableLayout, draw_conflicts,
    draw_console, draw_dialog, draw_main_tabs, draw_overlay, draw_panel_title, draw_picker,
    draw_rule, draw_small_terminal, draw_table, text,
};

const MIN_WIDTH: u16 = 80;
const MIN_HEIGHT: u16 = 24;
/// A screen with this height has a rule under the tabs and a title in each panel.
const TALL_HEIGHT: u16 = 30;
/// A console without a connection takes its identity from the end of the range,
/// and the manager gives identities from the start.
const FIRST_UNBOUND_CONSOLE: ConsoleId = u64::MAX;
const EVENTS_PER_FRAME: usize = 64;
/// The time of one step of the row search, so that a key waits less than a frame.
const SEARCH_SLICE: Duration = Duration::from_millis(8);
/// The time between two frames of a run. The time on screen needs this rate.
const RUN_FRAME: Duration = Duration::from_millis(100);
/// The frames that each picture of the mark of a run stays.
const FRAMES_PER_PICTURE: usize = 2;
/// A message of the hint bar stays this time, unless the user acts first.
const STATUS_TIME: Duration = Duration::from_secs(8);
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
    /// The folders of the files. Without a store, the application loads and saves nothing.
    pub store: Option<Store>,
    /// The folder where a project can start. The application asks before it creates one.
    pub offer_project: Option<PathBuf>,
    /// An explicit vault, such as an in-memory vault in tests. Native access stays lazy.
    pub vault: Option<Arc<dyn crate::creds::Vault>>,
    /// The `theme` and `icons` choices of the configuration.
    pub appearance: (Option<ThemeName>, Option<IconName>),
    /// The keys that accept a suggestion.
    pub suggestion_keys: SuggestionKeys,
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
            store: None,
            offer_project: None,
            vault: None,
            appearance: (None, None),
            suggestion_keys: SuggestionKeys::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    CreateProject,
    ReloadConfig,
    IgnoreConfigChange,
    /// Close the console, and drop the text that no file holds.
    DiscardAndClose(ConsoleId),
    DeleteScript(Scope, String),
    Associate(ConsoleId, ConnectionId),
    AuthCancel(u64),
    AuthConfirm(u64),
    AuthRetry(u64),
    AuthSession(u64),
    AuthPlain(u64),
    AuthMigrate(u64),
    AuthRemovePlain(u64),
    DeletePassword(Identity),
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

/// What a row of a list stands for.
enum PickerKind {
    /// The first row is the default of the connection, and the others are schemas.
    Schema { console: ConsoleId, connection: ConnectionId },
    /// The first row creates a console, and the others are the saved consoles.
    Consoles { connection: ConnectionId, scripts: Vec<ScriptEntry> },
    Orphans { scripts: Vec<ScriptEntry> },
    Associate { console: ConsoleId, connections: Vec<ConnectionId> },
}

/// A list of rows, with one chosen by the cursor.
struct Picker {
    kind: PickerKind,
    title: String,
    items: Vec<String>,
    cursor: usize,
    loading: bool,
}

/// The screen areas of the last frame. The mouse uses them.
#[derive(Clone, Debug, Default)]
struct ScreenLayout {
    navigator_title: Rect,
    /// The rows of the tree, without its title.
    navigator: Rect,
    divider: Option<u16>,
    main_tabs: Vec<MainTab>,
    body: Rect,
    console: ConsoleLayout,
    table: TableLayout,
    form: FormLayout,
    conflicts: ConflictLayout,
    password: Vec<(usize, Rect)>,
    /// The options of the open dialog, with the row of those options.
    dialog: (Vec<(u16, u16, usize)>, u16),
    /// The area of the open list, with the index of its first row.
    picker: (Rect, usize),
    /// The list of the command palette, with the index of its first row.
    palette: (Rect, usize),
}

pub struct App {
    consoles: Vec<Console>,
    /// The console that the console commands use. A table tab can be on screen.
    active: usize,
    /// The table tabs. The tab row mixes them with the consoles in the order of opening.
    tables: Vec<TableTab>,
    /// The count of opened tabs.
    tabs_opened: u64,
    /// The table tab on screen, or `None` when the active console is on screen.
    table_active: Option<usize>,
    connections: Vec<Connection>,
    navigator: Navigator,
    navigator_focused: bool,
    /// The catalog of a console without a connection.
    fallback_catalog: Option<Catalog>,
    /// The count of catalog reads. A new `Connection` keeps its id, so one count serves all.
    catalog_reads: u64,
    manager: Option<Manager>,
    events: Option<Receiver<ExecEvent>>,
    budget: Arc<Budget>,
    settings: Settings,
    keyboard: KeyboardMode,
    keymap: Keymap,
    suggestion_keys: SuggestionKeys,
    database: Option<PathBuf>,
    clipboard: SharedClipboard,
    /// The theme of the terminal, before the choices of the configuration.
    terminal_theme: Theme,
    color_env: ColorEnv,
    theme: Theme,
    layout: ScreenLayout,
    editor_ratio: u16,
    /// The first dialog is open. Another console can add a question at any time.
    dialogs: VecDeque<Dialog>,
    picker: Option<Picker>,
    palette: Option<Palette>,
    form: Option<ConnectionForm>,
    overlay: Option<Overlay>,
    status: Option<String>,
    /// The message on the clock, with the time of its first frame.
    status_shown: Option<(String, Instant)>,
    /// The start of the application, for the pictures of the mark of a run.
    started: Instant,
    /// The time of the last step of the main loop. The screen uses it for each run.
    clock: Instant,
    run_frame: usize,
    /// The request for a new name of a console.
    rename: Option<NamePrompt>,
    settings_form: Option<SettingsForm>,
    /// The files of the application. It is `None` when nothing loads or saves.
    persist: Option<persist::Persist>,
    auth: auth::Auth,
    /// The scripts that another program changed, and that wait for a choice.
    conflicts: Option<conflicts::Conflicts>,
    /// The SQL rules of a console that has no connection.
    fallback_dialect: Dialect,
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
        let mut options = options;
        let (persist, stored) = match persist::Persist::start(&mut options) {
            Some((persist, stored)) => (Some(persist), stored),
            None => (None, Vec::new()),
        };
        let dialect = match (&options.connector, options.connections.first(), &options.database) {
            (Some(connector), ..) => connector.dialect(),
            (None, _, Some(_)) => Dialect::Sqlite,
            (None, Some(definition), None) => definition.connector().dialect(),
            (None, None, None) => {
                stored.first().map_or(options.dialect, |item| item.definition.connector().dialect())
            }
        };
        let budget = Arc::new(Budget::new(options.settings.budget_bytes));
        let keymap = options.keymap.unwrap_or_else(|| Keymap::new(options.keyboard));
        let plain = persist.as_ref().map(|persist| PlainFile::new(&persist.store.global));
        let color_env = ColorEnv::read();
        let (chosen_theme, chosen_icons) = options.appearance;
        let mut app = Self {
            consoles: Vec::new(),
            active: 0,
            tables: Vec::new(),
            tabs_opened: 0,
            table_active: None,
            connections: Vec::new(),
            navigator: Navigator::default(),
            navigator_focused: false,
            fallback_catalog: options.catalog,
            catalog_reads: 0,
            manager: None,
            events: None,
            budget,
            settings: options.settings,
            keyboard: options.keyboard,
            keymap,
            suggestion_keys: options.suggestion_keys,
            database: options.database,
            clipboard,
            terminal_theme: theme,
            color_env,
            theme: theme.with_choices(chosen_theme, chosen_icons, color_env),
            layout: ScreenLayout::default(),
            editor_ratio: DEFAULT_EDITOR_RATIO,
            dialogs: VecDeque::new(),
            picker: None,
            palette: None,
            form: None,
            overlay: None,
            rename: None,
            settings_form: None,
            status: None,
            status_shown: None,
            started: Instant::now(),
            clock: Instant::now(),
            run_frame: 0,
            persist,
            auth: auth::Auth::new(options.vault.take(), plain),
            conflicts: None,
            fallback_dialect: options.dialect,
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
        for item in stored {
            app.add_stored(item);
        }
        if let Some(connection) = app.connections.first().map(|connection| connection.id) {
            app.bind_console(0, connection);
        }
        app.finish_start();
        app
    }

    fn unbound_console(&mut self, dialect: Dialect) -> Console {
        let editor = SqlEditor::new("", self.keyboard, dialect, self.clipboard.clone());
        let name = text::console_name(self.next_console_number);
        self.next_console_number += 1;
        let id = self.next_unbound_console;
        self.next_unbound_console -= 1;
        let mut console = Console::new(id, name, editor);
        console.opened = self.next_tab_place();
        console
    }

    fn next_tab_place(&mut self) -> u64 {
        self.tabs_opened += 1;
        self.tabs_opened
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Ends the runtime without a wait for a statement that does not answer.
    pub fn shutdown(&mut self) {
        self.auth.shutdown();
        self.save_session();
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
        if ends_status(&event) {
            self.clear_status();
        }
        self.route_event(event);
        // The application keeps the message of the editor, so one clock serves both.
        if let Some(message) = self.console_mut().editor.take_status() {
            self.status = Some(message.to_owned());
        }
    }

    /// Moves the clock of the screen. It returns true when a run needs a new frame.
    fn advance_clock(&mut self, now: Instant) -> bool {
        self.clock = now;
        let time = now.saturating_duration_since(self.started);
        let frame = usize::try_from(time.as_millis() / RUN_FRAME.as_millis()).unwrap_or(0);
        let running = self.consoles.iter().any(Console::is_busy) || self.tables.iter().any(TableTab::is_busy);
        let changed = running && frame != self.run_frame;
        self.run_frame = frame;
        changed
    }

    fn clear_status(&mut self) {
        self.status = None;
        self.status_shown = None;
    }

    /// Starts the time of a new message, and removes a message at the end of its time.
    /// It returns true when a message goes.
    fn expire_status(&mut self, now: Instant) -> bool {
        let Some(status) = &self.status else {
            self.status_shown = None;
            return false;
        };
        match &self.status_shown {
            Some((shown, since)) if shown == status => {
                let done = now.saturating_duration_since(*since) >= STATUS_TIME;
                if done {
                    self.clear_status();
                }
                done
            }
            _ => {
                self.status_shown = Some((status.clone(), now));
                false
            }
        }
    }

    fn route_event(&mut self, event: Event) {
        let blocked = !self.dialogs.is_empty()
            || self.conflicts.is_some()
            || self.overlay.is_some()
            || self.picker.is_some()
            || self.palette.is_some()
            || self.rename.is_some()
            || self.settings_form.is_some()
            || self.auth.prompt.is_some();
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) if self.dialogs.is_empty() && self.auth.prompt.is_some() => {
                if let Some(prompt) = &mut self.auth.prompt { prompt.paste(&text); }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.rename.is_some() => {
                if let Some(prompt) = &mut self.rename { prompt.paste(&text); }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.settings_form.is_some() => {
                if let Some(form) = &mut self.settings_form { form.paste(&text); }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.palette.is_some() => {
                if let Some(palette) = &mut self.palette { palette.paste(&text); }
            }
            Event::Paste(text) if !blocked => self.on_paste(&text),
            Event::Mouse(mouse) if !self.dialogs.is_empty() => self.on_dialog_mouse(mouse),
            Event::Mouse(mouse) if self.auth.prompt.is_some() => self.on_password_mouse(mouse),
            Event::Mouse(mouse) if self.conflicts.is_some() => self.on_conflicts_mouse(mouse),
            Event::Mouse(mouse) if self.picker.is_some() => self.on_picker_mouse(mouse),
            Event::Mouse(mouse) if self.palette.is_some() => self.on_palette_mouse(mouse),
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
        let ticked = self.tick();
        let authorized = self.poll_auth();
        let changed = self.has_work() || ticked || authorized;
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
            ExecEvent::CatalogLoaded { connection, read, catalog } => {
                self.on_catalog_loaded(connection, read, catalog);
            }
            ExecEvent::ColumnsLoaded { connection, relation, columns } => {
                self.on_columns_loaded(connection, &relation, columns);
            }
            ExecEvent::ConnectionTested { test, result } => self.on_connection_tested(test, result),
            ExecEvent::Disconnected { connection } => self.on_disconnected(connection),
            ExecEvent::PasswordRefused { connection, pool } => {
                crate::log::event("password refused");
                self.on_password_refused(connection, pool);
            }
            event => {
                if let ExecEvent::ExecutionFinished { end, .. } = &event {
                    crate::log::event(&format!("execution finished: {end:?}"));
                }
                self.on_console_event(event);
            }
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
        frame.render_widget(Block::new().style(self.theme.base), area);
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            draw_small_terminal(frame, area, (MIN_WIDTH, MIN_HEIGHT), &self.theme);
            return;
        }
        let tall = area.height >= TALL_HEIGHT;
        let [tabs, rule, body, status, hints] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(u16::from(tall)),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        self.layout.body = body;
        let views = self.tab_views();
        self.layout.main_tabs = draw_main_tabs(frame, tabs, views, self.active_tab(), &self.theme);
        if tall {
            draw_rule(frame, rule, &self.theme);
        }
        let main = self.draw_side(frame, body);
        if self.table_active.is_some() {
            self.draw_active_table(frame, main, tall);
        } else {
            self.draw_active_console(frame, main, tall);
        }
        self.draw_status(frame, status);
        self.draw_hint(frame, hints);
        self.draw_layers(frame, Rect { y: 0, height: status.y, ..area });
    }

    /// Draws the navigator and its divider. It returns the area for the console.
    fn draw_side(&mut self, frame: &mut Frame, body: Rect) -> Rect {
        if self.navigator.collapsed {
            self.layout.navigator_title = Rect::default();
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
        let [title, tree] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(side);
        let focused = self.navigator_focused;
        let connected: Vec<bool> = self
            .connections
            .iter()
            .map(|connection| {
                self.manager.as_ref().is_some_and(|manager| manager.is_connected(connection.id))
            })
            .collect();
        frame.buffer_mut().set_style(side, self.theme.panel);
        draw_panel_title(frame, title, text::TITLE_EXPLORER, focused, &self.theme);
        draw_navigator(frame, tree, &mut self.navigator, &self.connections, &connected, focused, &self.theme);
        let line = vec![Line::raw("│"); usize::from(divider.height)];
        frame.render_widget(Paragraph::new(line).style(self.theme.divider), divider);
        self.layout.navigator_title = title;
        self.layout.navigator = tree;
        self.layout.divider = Some(divider.x);
        main
    }

    fn draw_active_console(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let connection = self.active_connection();
        let name = connection.map_or(text::NO_DATABASE_LABEL, |connection| &connection.name).to_owned();
        let namespace = connection.map_or(Namespace::None, |item| item.capabilities.namespace);
        let transactions = connection.is_none_or(|item| item.capabilities.transactions);
        let default_schema = connection.and_then(|item| item.default_schema.clone());
        let bar = BarData {
            connection: &name,
            namespace,
            default_schema: default_schema.as_deref(),
            transactions,
            title: tall,
            welcome: self.welcome(),
        };
        let ratio = self.editor_ratio;
        let focused = !self.navigator_focused;
        let console = &mut self.consoles[self.active];
        self.layout.console = draw_console(frame, area, console, &bar, ratio, focused, &self.theme);
    }

    fn draw_active_table(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let Some(index) = self.table_active else { return };
        let connection = self.tables[index].connection;
        let name = self.connection(connection).map_or_else(String::new, |item| item.name.clone());
        let bar = TableBar { connection: &name, blocked: self.tables[index].is_blocked(&self.budget), title: tall };
        let focused = !self.navigator_focused;
        self.layout.table = draw_table(frame, area, &mut self.tables[index], &bar, focused, &self.theme);
    }

    /// Returns the name of the place that receives keys, for the status line.
    fn status_panel(&self) -> StatusPanel {
        if self.navigator_focused {
            return StatusPanel::Explorer;
        }
        if let Some(tab) = self.table() {
            return if tab.focus == TableFocus::Grid { StatusPanel::Table } else { StatusPanel::Filter };
        }
        let console = self.console();
        if console.focus == Focus::Results {
            return StatusPanel::Results;
        }
        if !self.vim() {
            return StatusPanel::Editor;
        }
        match console.editor.state.mode {
            EditorMode::Insert => StatusPanel::Insert,
            EditorMode::Visual => StatusPanel::Visual,
            _ => StatusPanel::Normal,
        }
    }

    fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let panel = self.status_panel();
        let data = (self.budget.used(), self.budget.limit());
        if let Some(tab) = self.table() {
            let view = tab.result.view;
            let position = (!tab.result.table.rows.is_empty()).then_some((view.row + 1, view.col + 1));
            let connection = self.connection(tab.connection).map_or("", |item| item.name.as_str());
            let status = StatusData {
                panel,
                connection,
                schema: Some(&tab.relation.schema),
                session: None,
                phase: tab.phase,
                run_time: run_time(tab.phase, &tab.run, self.clock),
                pool: self.manager.as_ref().map(|manager| manager.usage(tab.connection)),
                data,
                position,
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        let console = self.console();
        let connection = self.active_connection();
        let namespace = connection.map_or(Namespace::None, |item| item.capabilities.namespace);
        let default_schema = connection.and_then(|item| item.default_schema.as_deref());
        let result = console.results.get(console.active).filter(|_| console.focus == Focus::Results);
        let position = match result {
            Some(result) => {
                (!result.table.rows.is_empty()).then_some((result.view.row + 1, result.view.col + 1))
            }
            None => {
                let cursor = console.editor.cursor();
                Some((cursor.row + 1, cursor.col + 1))
            }
        };
        let status = StatusData {
            panel,
            connection: connection.map_or(text::NO_DATABASE_LABEL, |item| item.name.as_str()),
            schema: (namespace != Namespace::None).then(|| console.schema.as_deref().or(default_schema)).flatten(),
            session: Some((console.manual, console.transaction)),
            phase: console.phase,
            run_time: run_time(console.phase, &console.run, self.clock),
            pool: console.connection.zip(self.manager.as_ref()).map(|(id, manager)| manager.usage(id)),
            data,
            position,
        };
        draw_statusline(frame, area, &status, &self.theme);
    }

    fn draw_hint(&self, frame: &mut Frame, area: Rect) {
        let line = match self.status.as_deref().or_else(|| self.auth_wait()) {
            Some(status) => Line::styled(status.to_owned(), self.theme.status),
            None => hint_line(&self.hint_items(), &self.theme),
        };
        // The row starts one column after the edge, as the status line does.
        let area = Rect { x: area.x + 1, width: area.width.saturating_sub(1), ..area };
        frame.render_widget(Paragraph::new(line), area);
    }

    /// Draws the windows above the panels. `bounds` holds the rows above the status line.
    fn draw_layers(&mut self, frame: &mut Frame, bounds: Rect) {
        if self.pending_key.is_some_and(Chord::is_prefix) {
            draw_which_key(frame, bounds, &self.prefix_menu(), &self.theme);
        }
        if let Some(form) = &self.form {
            self.layout.form = draw_form(frame, form, &self.theme);
        }
        if let Some(picker) = &self.picker {
            let items: Vec<&str> = picker.items.iter().map(String::as_str).collect();
            self.layout.picker =
                draw_picker(frame, &picker.title, &items, picker.cursor, picker.loading, &self.theme);
        }
        if let Some(conflicts) = &self.conflicts {
            let rows: Vec<ConflictRowView> = conflicts
                .rows
                .iter()
                .map(|row| ConflictRowView {
                    name: &row.name,
                    removed: row.removed,
                    choice: row.choice.map(|choice| row.options()[choice]),
                })
                .collect();
            self.layout.conflicts =
                draw_conflicts(frame, &rows, conflicts.cursor, conflicts.note, &self.theme);
        }
        if let Some(overlay) = &mut self.overlay {
            let Overlay { title, contents, footer, scroll } = overlay;
            *scroll = draw_overlay(frame, title, contents, footer, *scroll, &self.theme);
        }
        if let Some(palette) = &self.palette {
            self.layout.palette = draw_palette(frame, palette, &self.theme);
        }
        if let Some(form) = &self.settings_form {
            draw_settings(frame, form, &self.theme);
        }
        if let Some(prompt) = &self.rename {
            draw_name(frame, prompt, &self.theme);
        }
        if let Some(prompt) = &self.auth.prompt {
            self.layout.password = crate::ui::auth::draw_password(frame, prompt, &self.theme);
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
    let report_folder = options.store.as_ref().map(|store| store.global.dir.clone());
    // The start of the terminal can wait for an answer, and a signal can arrive meanwhile.
    let signal = signal_flag()?;
    let mut session = Session::start(report_folder)?;
    let mut app = App::new(options, clipboard, Theme::from_env());
    let result = catch_unwind(AssertUnwindSafe(|| event_loop(&mut app, &mut session, &signal)));
    match result {
        Ok(Ok(())) => Ok(()),
        // A closed terminal ends the loop with an error, and the text still needs a place.
        Ok(Err(error)) => {
            app.save_without_questions();
            app.shutdown();
            Err(error)
        }
        // The panic hook restored the terminal and wrote the report. The text comes next.
        Err(panic) => {
            let _ = catch_unwind(AssertUnwindSafe(|| app.save_without_questions()));
            resume_unwind(panic);
        }
    }
}

/// Sets a flag for each signal that ends a process.
fn signal_flag() -> io::Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
        for signal in [SIGHUP, SIGINT, SIGQUIT, SIGTERM] {
            signal_hook::flag::register(signal, flag.clone())?;
        }
    }
    Ok(flag)
}

/// Returns true for an action of the user. A move of the pointer and the wheel are not
/// actions, because they only let the user look at the screen.
fn ends_status(event: &Event) -> bool {
    match event {
        Event::Key(key) => key.kind != KeyEventKind::Release,
        Event::Mouse(mouse) => matches!(mouse.kind, MouseEventKind::Down(_)),
        Event::Paste(_) => true,
        _ => false,
    }
}

fn event_loop(app: &mut App, session: &mut Session, signal: &AtomicBool) -> io::Result<()> {
    let mut dirty = true;
    loop {
        // A signal comes first, because a closed terminal cannot draw.
        if signal.load(Ordering::Relaxed) {
            crate::log::event("signal");
            app.save_without_questions();
            app.shutdown();
            return Ok(());
        }
        if dirty {
            session.screen.draw(|frame| app.draw(frame))?;
            dirty = false;
        }
        if app.should_quit() {
            crate::log::event("exit");
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

