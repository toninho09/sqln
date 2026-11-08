//! The application shell: the consoles, the connections, and the screen.

mod agent;
mod ai;
mod analysis;
mod auth;
mod auto_refresh;
mod browse;
mod close;
mod commands;
mod compare;
mod conflicts;
mod connections;
mod data_edit;
mod definition;
mod diagram;
mod export;
mod guard;
mod hints;
mod history;
mod import;
mod keys;
mod mouse;
mod persist;
mod review;
mod scripts;
mod session;
mod sql_file;
mod tables;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::PathBuf;
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

use self::keys::RunTarget;
use self::mouse::Drag;
use crate::browser::FileBrowser;
use crate::catalog::{Catalog, RelationRef};
use crate::clipboard::{SharedClipboard, SystemClipboard};
use crate::compare::form::CompareForm;
use crate::compare::tab::CompareTab;
use crate::connection::{Connection, Definition};
use crate::connector::{Connector, Namespace, TransactionCommand};
use crate::console::{AfterTransaction, Console, Focus, Phase};
use crate::creds::{Identity, PlainFile};
use crate::definition::Target;
use crate::diagram::tab::DiagramTab;
use crate::editor::{DEFAULT_UNDO_LIMIT, KeyboardMode, SqlEditor};
use crate::exec::{ConnectionId, ConsoleId, ExecEvent, Manager, Settings};
use crate::export::ExportChoices;
use crate::export::form::ExportForm;
use crate::form::ConnectionForm;
use crate::generate::Template;
use crate::grid::Budget;
use crate::help::Help;
use crate::history::HistoryList;
use crate::import::ImportChoices;
use crate::import::form::ImportForm;
use crate::input::{Chord, Keymap, Region, SuggestionKeys};
use crate::navigator::Navigator;
use crate::palette::Palette;
use crate::settings::SettingsForm;
use crate::sql::Dialect;
use crate::store::config::{IconName, ThemeName};
use crate::store::scripts::ScriptEntry;
use crate::store::{Scope, Store};
use crate::structure::SessionSignal;
use crate::table::{Follow, TableFocus, TableTab};
use crate::terminal::Session;
use crate::terminal::{ring_bell, watch_terminal};
use crate::ui::browser::draw_browser;
use crate::ui::compare::{CompareLayout, draw_compare, draw_compare_form};
use crate::ui::console::BarData;
use crate::ui::diagram::{DiagramBar, draw_diagram};
use crate::ui::export::{ExportLayout, draw_export};
use crate::ui::form::{FormLayout, draw_form};
use crate::ui::help::draw_help;
use crate::ui::history::draw_history;
use crate::ui::import::{ImportLayout, draw_import};
use crate::ui::kit::feedback::hint_line;
use crate::ui::kit::panel::{draw_panel_title, draw_rule};
use crate::ui::name::{NamePrompt, draw_name};
use crate::ui::navigator::draw_navigator;
use crate::ui::palette::draw_palette;
use crate::ui::settings::{SettingsLayout, draw_settings};
use crate::ui::statusline::{StatusData, StatusPanel, draw_statusline, run_time};
use crate::ui::theme::{ColorEnv, Theme, ThemeCatalog};
use crate::ui::welcome::draw_welcome;
use crate::ui::which_key::draw_which_key;
use crate::ui::wrap::WrappedText;
use crate::ui::{
    ConflictLayout, ConflictRowView, ConsoleLayout, MainTab, TableBar, TableLayout, draw_conflicts,
    draw_console, draw_dialog, draw_main_tabs, draw_overlay, draw_picker, draw_small_terminal,
    draw_table, text,
};

const MIN_WIDTH: u16 = 80;
const MIN_HEIGHT: u16 = 24;
/// A screen with this height has a rule under the tabs and a title in each panel.
const TALL_HEIGHT: u16 = 30;
/// A console without a connection takes its identity from the end of the range,
/// and the manager gives identities from the start.
const FIRST_UNBOUND_CONSOLE: ConsoleId = u64::MAX;
/// The time of one step of work between two frames, so that a key waits less than a frame.
const WORK_SLICE: Duration = Duration::from_millis(8);
/// The time between two frames of a run. The time on screen needs this rate.
const RUN_FRAME: Duration = Duration::from_millis(100);
/// The frames that each picture of the mark of a run stays.
const FRAMES_PER_PICTURE: usize = 2;
/// A message of the hint bar stays this time, unless the user acts first.
const STATUS_TIME: Duration = Duration::from_secs(8);
/// The time that the main loop waits for a key when no work waits.
const KEY_WAIT: Duration = Duration::from_millis(20);
const DEFAULT_EDITOR_RATIO: u16 = 50;
const MIN_EDITOR_RATIO: u16 = 10;
const MAX_EDITOR_RATIO: u16 = 90;

pub struct Options {
    pub ai_completion: crate::ai::Preferences,
    pub ai_vault: Option<Arc<crate::creds::SystemVault>>,
    /// Starts the local assistant endpoint. The command-line interface enables it.
    pub agent_ipc: bool,
    pub keyboard: KeyboardMode,
    /// The limit of the undo history of one console, in the conventional mode.
    pub undo_limit: usize,
    /// The key map. Without a value, the defaults of `keyboard` apply.
    pub keymap: Option<Keymap>,
    pub dialect: Dialect,
    pub catalog: Option<Catalog>,
    pub database: Option<PathBuf>,
    /// The connections of the command line. The first console uses the first one.
    pub connections: Vec<Definition>,
    /// True when the command line names the database. The tabs of the last session
    /// stay closed and stay in their file, and the start offers no project.
    pub session_target: bool,
    /// True when a start without a connection and without a tab of the last session
    /// shows the home screen. `main` sets it. Without it, the start keeps an empty
    /// console, as a test that types SQL with no connection needs.
    pub start_home: bool,
    /// A ready connector. It has priority over `database`.
    pub connector: Option<Arc<dyn Connector>>,
    /// The name of `connector` in the interface.
    pub label: Option<String>,
    /// A `.sql` file that the command line opens as a console at the start.
    pub script: Option<PathBuf>,
    pub settings: Settings,
    /// The folders of the files. Without a store, the application loads and saves nothing.
    pub store: Option<Store>,
    /// The folder where a project can start. The application asks before it creates one.
    pub offer_project: Option<PathBuf>,
    /// An explicit vault, such as an in-memory vault in tests. Native access stays lazy.
    pub vault: Option<Arc<dyn crate::creds::Vault>>,
    /// The `theme` and `icons` choices of the configuration.
    pub appearance: (Option<ThemeName>, Option<IconName>),
    /// The custom themes of the files of both scopes.
    pub custom_themes: ThemeCatalog,
    /// The trouble of the theme files at the start.
    pub theme_notes: Vec<String>,
    /// The keys that accept a suggestion.
    pub suggestion_keys: SuggestionKeys,
    /// True when Messages shows internal and shared commands.
    pub show_system_commands: bool,
    /// The MCP preferences of the configuration.
    pub agent: crate::store::config::AgentSettings,
    pub export_choices: ExportChoices,
    pub import_choices: ImportChoices,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            agent_ipc: false,
            keyboard: KeyboardMode::Conventional,
            undo_limit: DEFAULT_UNDO_LIMIT,
            keymap: None,
            dialect: Dialect::Postgres,
            catalog: None,
            database: None,
            connections: Vec::new(),
            session_target: false,
            start_home: false,
            connector: None,
            label: None,
            script: None,
            settings: Settings::default(),
            store: None,
            offer_project: None,
            vault: None,
            appearance: (None, None),
            custom_themes: ThemeCatalog::default(),
            theme_notes: Vec::new(),
            suggestion_keys: SuggestionKeys::default(),
            ai_completion: crate::ai::Preferences::default(),
            ai_vault: None,
            show_system_commands: crate::store::config::DEFAULT_SHOW_SYSTEM_COMMANDS,
            agent: crate::store::config::AgentSettings::default(),
            export_choices: ExportChoices::default(),
            import_choices: ImportChoices::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DialogAction {
    AiFormRetry,
    AiFormSession,
    AiAllow(u64),
    AiCancel(u64),
    AiRetry(u64),
    AiSession(u64),
    AgentAccess(String, agent::Grant),
    AgentRevoke(String),
    AgentDirect(ConsoleId),
    /// Removes the remembered access of a client.
    AgentForget(String),
    /// The answer to the run question of an agent task.
    AgentRun(String, bool),
    /// Stops one agent task. Its session keeps its access.
    AgentStopTask(String),
    /// Revokes the session of its last agent tab, then closes the tab.
    AgentCloseRevoke(ConsoleId),
    Dismiss,
    CreateDatabase,
    CreateFormFile,
    StopAndClose(ConsoleId),
    CancelClose(ConsoleId),
    Transaction(ConsoleId, TransactionCommand, AfterTransaction),
    KeepWaiting(ConsoleId),
    CloseConnection(ConsoleId),
    RunAnyway(ConsoleId, RunTarget),
    /// Run the statements that the question of `run_warning` showed.
    RunConfirmed(ConsoleId, RunTarget),
    /// The interval of the auto-refresh of the tab on screen. `None` turns it off.
    AutoRefresh(Option<u64>),
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
    /// Replace the file of the export dialog.
    ExportReplace,
    /// Start the import that the question of production showed.
    ImportConfirmed,
    ApplyEdits(ConsoleId),
    ReviewEdits(ConsoleId),
    DiscardEdits(ConsoleId),
    CancelEdits(ConsoleId),
    /// Send the signal that the question of a sessions tab showed.
    SessionSignal(ConsoleId, SessionSignal, u64),
    /// Give the incomplete file of a tab its final name.
    ExportKeep(ConsoleId),
    ExportDelete(ConsoleId),
    /// Opens the pending `.sql` file as a copy or as a linked console.
    SqlFileCancel,
    SqlFileCopy,
    SqlFileLinked,
    /// The answers of the questions of a console that links to a file.
    SqlFileReload(ConsoleId),
    SqlFileOverwrite(ConsoleId),
    SqlFileWriteClose(ConsoleId),
    SqlFileDiscardClose(ConsoleId),
}

struct Dialog {
    title: String,
    body: String,
    options: Vec<(&'static str, DialogAction)>,
    focus: usize,
}

struct Overlay {
    _analysis_memory: Option<crate::analysis::Memory>,
    title: String,
    contents: WrappedText,
    footer: String,
    scroll: usize,
    /// The other form of a JSON value, with its footer. `f` shows it.
    other: Option<(String, String)>,
    /// The row of the grid that a row window shows. The side keys change it.
    row: Option<usize>,
}

impl Overlay {
    /// Shows the other form of a JSON value from its first row.
    fn switch_form(&mut self) {
        if let Some((text, footer)) = self.other.take() {
            let shown = self.contents.to_text();
            let old_footer = std::mem::replace(&mut self.footer, footer);
            self.contents = WrappedText::new(text);
            self.other = Some((shown, old_footer));
            self.scroll = 0;
        }
    }
}

/// What a row of a list stands for.
enum PickerKind {
    Analysis(analysis::Choice),
    Agent(Vec<agent::Choice>),
    /// The first row is the default of the connection, and the others are schemas.
    Schema {
        console: ConsoleId,
        connection: ConnectionId,
    },
    /// The first row creates a console, and the others are the saved consoles.
    Consoles {
        connection: ConnectionId,
        scripts: Vec<ScriptEntry>,
    },
    Orphans {
        scripts: Vec<ScriptEntry>,
    },
    Associate {
        console: ConsoleId,
        connections: Vec<ConnectionId>,
    },
    /// The connection of a `.sql` file that opens as a console. `path` is `None` when
    /// the file browser chooses the file after the connection.
    SqlFile {
        path: Option<std::path::PathBuf>,
        connections: Vec<ConnectionId>,
    },
    /// The rows are the SQL templates of a relation.
    /// The keys of a row of a table tab, after the key `F`.
    Follow {
        connection: ConnectionId,
        choices: Vec<Follow>,
    },
    /// The templates of a relation, then the DDL entry of a relation or an object.
    Generate {
        request: Option<SqlRequest>,
        templates: Vec<Template>,
        definition: Option<(ConnectionId, Target)>,
    },
}

/// The relation of a SQL template.
#[derive(Clone)]
struct SqlRequest {
    connection: ConnectionId,
    relation: RelationRef,
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
    /// The field of the filter of the tree.
    navigator_filter: Rect,
    /// The rows of the tree, without its title and without the field of its filter.
    navigator: Rect,
    divider: Option<u16>,
    main_tabs: Vec<MainTab>,
    /// The **Quit** button at the right end of the main tabs.
    quit: Rect,
    agent_bar: crate::ui::agent::BarTargets,
    agent_card: crate::ui::agent::CardTargets,
    /// The item of the card in the last frame. A click acts on this item.
    agent_notice: Option<agent::Notice>,
    body: Rect,
    console: ConsoleLayout,
    table: TableLayout,
    /// The picture of the diagram on screen.
    diagram: Rect,
    /// The grid of the compare on screen.
    compare: Rect,
    compare_form: CompareLayout,
    form: FormLayout,
    settings: SettingsLayout,
    export: ExportLayout,
    import: ImportLayout,
    edit: crate::ui::data_edit::EditLayout,
    conflicts: ConflictLayout,
    password: Vec<(usize, Rect)>,
    /// The options of the open dialog, with the row of those options.
    dialog: (Vec<(u16, u16, usize)>, u16),
    /// The area of the open list, with the index of its first row.
    picker: (Rect, usize),
    /// The list of the command palette, with the index of its first row.
    palette: (Rect, usize),
    /// The list of the query history, with the index of its first row.
    history: (Rect, usize),
    /// The list of the file browser, with the index of its first row.
    browser: (Rect, usize),
}

pub struct App {
    ai: ai::State,
    consoles: Vec<Console>,
    /// The console that the console commands use. A table tab can be on screen.
    active: usize,
    /// The table tabs. The tab row mixes them with the consoles in the order of opening.
    tables: Vec<TableTab>,
    /// The count of opened tabs.
    tabs_opened: u64,
    /// The table tab on screen, or `None` when the active console is on screen.
    table_active: Option<usize>,
    /// The diagram tabs. The tab row mixes them with the other tabs in the order of opening.
    diagrams: Vec<DiagramTab>,
    /// The diagram tab on screen. At most one of `table_active`, this, and
    /// `compare_active` has a value.
    diagram_active: Option<usize>,
    /// The compare tabs. The tab row mixes them with the other tabs in the order of opening.
    compares: Vec<CompareTab>,
    /// The compare tab on screen.
    compare_active: Option<usize>,
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
    undo_limit: usize,
    keymap: Keymap,
    suggestion_keys: SuggestionKeys,
    show_system_commands: bool,
    database: Option<PathBuf>,
    clipboard: SharedClipboard,
    /// The theme of the terminal, before the choices of the configuration.
    terminal_theme: Theme,
    color_env: ColorEnv,
    /// The `theme` and `icons` choices of the files.
    appearance: (Option<ThemeName>, Option<IconName>),
    /// The custom themes of both scopes, after the precedence.
    custom_themes: ThemeCatalog,
    /// The theme of the current frame. `choose_theme` writes it at the start of each
    /// frame, so a change of tab or of a choice needs no other step.
    theme: Theme,
    layout: ScreenLayout,
    editor_ratio: u16,
    /// The first dialog is open. Another console can add a question at any time.
    dialogs: VecDeque<Dialog>,
    picker: Option<Picker>,
    palette: Option<Palette>,
    history: Option<HistoryList>,
    form: Option<ConnectionForm>,
    overlay: Option<Overlay>,
    analysis_job: Option<crate::analysis::Job>,
    help: Option<Help>,
    status: Option<String>,
    /// The message on the clock, with the time of its first frame.
    status_shown: Option<(String, Instant)>,
    /// The start of the application, for the pictures of the mark of a run.
    started: Instant,
    /// The time of the last step of the main loop. The screen uses it for each run.
    clock: Instant,
    run_frame: usize,
    /// The whole minutes of the open transaction on screen in the last frame.
    transaction_minute: Option<u64>,
    /// True when a long run ended and the main loop did not ring the bell yet.
    bell: bool,
    /// True when the last read found execution events.
    events_flow: bool,
    /// A SQL template that waits for the columns of its relation.
    pending_sql: Option<(SqlRequest, Template)>,
    /// The connections whose diagrams wait for their catalogs, which give the current
    /// schemas.
    pending_diagrams: Vec<ConnectionId>,
    /// The request for a new name of a console.
    rename: Option<NamePrompt>,
    settings_form: Option<SettingsForm>,
    export_form: Option<ExportForm>,
    import_form: Option<ImportForm>,
    edit_form: Option<crate::data_edit::form::EditForm>,
    compare_form: Option<CompareForm>,
    /// The file browser of the focused path field in a dialog or in the form.
    browser: Option<FileBrowser>,
    /// The connection that waits for the `.sql` file of an open.
    sql_file_open: Option<ConnectionId>,
    /// The file that waits for the answer of the copy-or-linked question.
    sql_file: Option<sql_file::PendingSqlFile>,
    /// The first choices of the next export dialog.
    export_choices: ExportChoices,
    /// The exports in progress, with at most one for each tab.
    exports: Vec<export::ExportRun>,
    /// The first choices of the next import dialog.
    import_choices: ImportChoices,
    /// The imports in progress, with at most one for each tab.
    imports: Vec<import::ImportRun>,
    /// The reads of a DDL in progress.
    definitions: Vec<definition::DefinitionRead>,
    /// A table tab whose key `F` waits for the keys of its table.
    pending_follow: Option<ConsoleId>,
    /// The answers of the file reads of the import dialog.
    import_probes: import::ProbeChannel,
    /// A table tab that opens the import dialog after its first page.
    import_wanted: Option<ConsoleId>,
    /// The files of the application. It is `None` when nothing loads or saves.
    persist: Option<persist::Persist>,
    auth: auth::Auth,
    agent: agent::State,
    /// The scripts that another program changed, and that wait for a choice.
    conflicts: Option<conflicts::Conflicts>,
    /// The SQL rules of a console that has no connection.
    fallback_dialect: Dialect,
    /// The region and keys of a sequence that needs more keys.
    pending_key: Option<(Region, Vec<Chord>)>,
    /// The region of the command page that waits for its next key.
    command_page: Option<Region>,
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
        let dialect = match (
            &options.connector,
            options.connections.first(),
            &options.database,
        ) {
            (Some(connector), ..) => connector.dialect(),
            (None, _, Some(_)) => Dialect::Sqlite,
            (None, Some(definition), None) => definition.connector().dialect(),
            (None, None, None) => stored.first().map_or(options.dialect, |item| {
                item.definition.connector().dialect()
            }),
        };
        let budget = Arc::new(Budget::new(options.settings.budget_bytes));
        let keymap = options
            .keymap
            .unwrap_or_else(|| Keymap::new(options.keyboard));
        let plain = persist
            .as_ref()
            .map(|persist| PlainFile::new(&persist.store.global));
        let color_env = ColorEnv::read();
        let mut app = Self {
            consoles: Vec::new(),
            active: 0,
            tables: Vec::new(),
            tabs_opened: 0,
            table_active: None,
            diagrams: Vec::new(),
            diagram_active: None,
            compares: Vec::new(),
            compare_active: None,
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
            undo_limit: options.undo_limit,
            keymap,
            suggestion_keys: options.suggestion_keys,
            ai: ai::State::new(options.ai_completion, options.ai_vault),
            show_system_commands: options.show_system_commands,
            database: options.database,
            clipboard,
            terminal_theme: theme,
            color_env,
            appearance: options.appearance,
            custom_themes: options.custom_themes.clone(),
            theme,
            layout: ScreenLayout::default(),
            editor_ratio: DEFAULT_EDITOR_RATIO,
            dialogs: VecDeque::new(),
            picker: None,
            palette: None,
            history: None,
            form: None,
            overlay: None,
            analysis_job: None,
            help: None,
            rename: None,
            settings_form: None,
            export_form: None,
            import_form: None,
            edit_form: None,
            compare_form: None,
            browser: None,
            sql_file_open: None,
            sql_file: None,
            export_choices: options.export_choices.clone(),
            exports: Vec::new(),
            import_choices: options.import_choices.clone(),
            imports: Vec::new(),
            definitions: Vec::new(),
            pending_follow: None,
            import_probes: import::ProbeChannel::default(),
            import_wanted: None,
            status: None,
            status_shown: None,
            started: Instant::now(),
            clock: Instant::now(),
            run_frame: 0,
            transaction_minute: None,
            bell: false,
            events_flow: false,
            pending_sql: None,
            pending_diagrams: Vec::new(),
            persist,
            auth: auth::Auth::new(options.vault.take(), plain),
            agent: agent::State::default(),
            conflicts: None,
            fallback_dialect: options.dialect,
            pending_key: None,
            command_page: None,
            drag: None,
            last_click: None,
            next_console_number: 1,
            next_unbound_console: FIRST_UNBOUND_CONSOLE,
            next_test: 1,
            exiting: false,
            quit: false,
        };
        let console = app.unbound_console(dialect);
        let first = console.id;
        app.consoles.push(console);
        let start_home = options.start_home && !options.session_target;
        if let Some(connector) = options.connector {
            let name = options
                .label
                .unwrap_or_else(|| text::DATABASE_LABEL.to_owned());
            app.add_connection(name, None, connector);
        } else {
            app.prepare_database();
        }
        for definition in options.connections {
            app.add_definition(definition);
        }
        // A file that does not exist binds the console after the answer of its dialog.
        let waits_for_target = options.session_target && app.connections.is_empty();
        for item in stored {
            app.add_stored(item);
        }
        if let Some(connection) = app.connections.first().map(|connection| connection.id)
            && !waits_for_target
        {
            app.bind_console(0, connection);
        }
        app.finish_start();
        // The empty console of the start stays only when no tab of the last session came
        // back. Without a connection, it gives its place to the home screen.
        if start_home
            && app.connections.is_empty()
            && let Some(index) = app.console_index(first)
        {
            app.consoles[index].hidden = true;
            app.focus_navigator();
        }
        app.agent.settings = options.agent;
        if options.agent_ipc {
            let directory =
                crate::agent::ipc::runtime_dir().map_err(|error| text::agent_endpoint(&error));
            app.agent.directory = Some(directory);
            app.open_agent_endpoint();
        }
        if let Some(path) = options.script.take() {
            app.open_sql_file_path(path);
        }
        // The choices of the configuration apply to the first frame, as in each later one.
        app.choose_theme();
        app.missing_theme_note();
        if !options.theme_notes.is_empty() {
            app.status = Some(options.theme_notes.join(" "));
        }
        app
    }

    /// Notes the default theme of the files when no file holds it.
    fn missing_theme_note(&mut self) {
        if let Some(ThemeName::Custom(id)) = &self.appearance.0
            && self.custom_themes.get(id).is_none()
        {
            self.status = Some(text::theme_missing(id));
        }
    }

    /// Each editor of a console comes from here, so it has the limit of the settings.
    fn new_editor(&self, text: &str, dialect: Dialect) -> SqlEditor {
        let mut editor = SqlEditor::new(text, self.keyboard, dialect, self.clipboard.clone());
        editor.set_undo_limit(self.undo_limit);
        editor
    }

    fn unbound_console(&mut self, dialect: Dialect) -> Console {
        let editor = self.new_editor("", dialect);
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
        self.agent.server.take();
        self.drain_agent();
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
        self.connections
            .iter()
            .find(|connection| connection.id == id)
    }

    /// Returns the catalog that a connection already read.
    fn catalog(&self, id: ConnectionId) -> Option<&Catalog> {
        Self::catalog_of(&self.connections, id)
    }

    /// Finds a catalog among the connections. A call of the borrowed fields serves the
    /// places that hold another field of `App` at the same time.
    fn catalog_of(connections: &[Connection], id: ConnectionId) -> Option<&Catalog> {
        connections
            .iter()
            .find(|connection| connection.id == id)?
            .catalog
            .as_ref()
    }

    fn connection_mut(&mut self, id: ConnectionId) -> Option<&mut Connection> {
        self.connections
            .iter_mut()
            .find(|connection| connection.id == id)
    }

    fn active_connection(&self) -> Option<&Connection> {
        self.console().connection.and_then(|id| self.connection(id))
    }

    /// Returns the connection of the open tab.
    fn tab_connection(&self) -> Option<&Connection> {
        if let Some(tab) = self.compare() {
            return self.connection(tab.sides[1].side.connection);
        }
        if let Some(tab) = self.diagram() {
            return self.connection(tab.connection);
        }
        match self.table() {
            Some(tab) => self.connection(tab.connection),
            None => self.active_connection(),
        }
    }

    /// Returns the theme of the next frame. An open dialog shows its choice before the
    /// save. After that, the connection of the open tab has priority over the default.
    fn theme_choice(&self) -> Option<ThemeName> {
        if let Some(form) = &self.settings_form {
            return form.preview_theme();
        }
        let chosen = self.form.as_ref().and_then(|form| form.theme.clone());
        let connection = || self.tab_connection()?.definition.as_ref()?.theme.clone();
        chosen
            .or_else(connection)
            .or_else(|| self.appearance.0.clone())
    }

    /// The palette of a custom id: the dialog of the settings comes first, because
    /// its drafts preview on the screen.
    fn draft_scheme(&self, id: &str) -> Option<crate::ui::schemes::Scheme> {
        if let Some(form) = &self.settings_form {
            if let Some(scheme) = form.theme_scheme(id) {
                return Some(scheme);
            }
            if form.theme_gone(id) {
                return None;
            }
        }
        self.custom_themes.scheme(id)
    }

    fn choose_theme(&mut self) {
        let name = self.theme_choice();
        let base = self.terminal_theme;
        let (_, icons) = self.appearance;
        let env = self.color_env;
        self.theme = base.with_choices(name, |id| self.draft_scheme(id), icons, env);
    }

    /// Adds a question. It opens now, or after the questions before it.
    fn show_dialog(
        &mut self,
        title: impl Into<String>,
        body: String,
        options: Vec<(&'static str, DialogAction)>,
    ) {
        self.dialogs.push_back(Dialog {
            title: title.into(),
            body,
            options,
            focus: 0,
        });
    }

    fn sync_command_history(&mut self) {
        let active = self
            .console_on_screen()
            .then(|| self.consoles[self.active].id);
        let Some(manager) = &mut self.manager else {
            return;
        };
        for console in &self.consoles {
            if let Some(connection) = console
                .connection
                .and_then(|id| self.connections.iter().find(|item| item.id == id))
            {
                manager.set_console_history(
                    console.id,
                    console.messages.id,
                    format!("{} [{}]", connection.name, connection.id),
                );
            }
        }
        manager.set_active_history(active);
    }

    pub fn on_event(&mut self, event: Event) {
        self.sync_command_history();
        if ends_status(&event) {
            self.clear_status();
        }
        self.keep_agent_focus();
        self.sync_ai();
        self.route_event(event);
        self.sync_ai();
        self.keep_agent_focus();
        self.sync_command_history();
        // The application keeps the message of the editor, so one clock serves both.
        if let Some(message) = self.console_mut().editor.take_status() {
            self.status = Some(message.to_owned());
        }
    }

    /// Keeps the focus of an agent tab without SQL on its results, because it shows no editor.
    fn keep_agent_focus(&mut self) {
        let Some(console) = self.consoles.get(self.active) else {
            return;
        };
        if console.focus != Focus::Results && self.agent_tab_without_sql(console.id) {
            self.consoles[self.active].focus = Focus::Results;
        }
        self.see_agent_sql();
    }

    /// Returns true for an agent tab that holds no SQL for the user. It shows no editor.
    fn agent_tab_without_sql(&self, console: ConsoleId) -> bool {
        self.agent.owns(console)
            && self
                .console_index(console)
                .is_some_and(|index| self.consoles[index].editor.is_blank())
    }

    /// Moves the clock of the screen. It returns true when a run or the age of an open
    /// transaction needs a new frame.
    fn advance_clock(&mut self, now: Instant) -> bool {
        self.clock = now;
        let time = now.saturating_duration_since(self.started);
        let frame = usize::try_from(time.as_millis() / RUN_FRAME.as_millis()).unwrap_or(0);
        // An agent task updates the time of its card, and the card appears after a delay.
        let running = self.ai.busy()
            || self.agent.active_tasks() != 0
            || self.consoles.iter().any(Console::is_busy)
            || self.tables.iter().any(TableTab::is_busy)
            || self.diagrams.iter().any(DiagramTab::is_busy)
            || self.compares.iter().any(CompareTab::is_busy);
        let changed = running && frame != self.run_frame;
        self.run_frame = frame;
        let minute = self
            .console()
            .transaction_age(now)
            .map(|age| age.as_secs() / 60);
        let aged = minute != self.transaction_minute;
        self.transaction_minute = minute;
        changed || aged
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

    fn modal_is_open(&self) -> bool {
        !self.dialogs.is_empty()
            || self.ai.modal()
            || self.conflicts.is_some()
            || self.overlay.is_some()
            || self.help.is_some()
            || self.picker.is_some()
            || self.palette.is_some()
            || self.history.is_some()
            || self.rename.is_some()
            || self.settings_form.is_some()
            || self.export_form.is_some()
            || self.import_form.is_some()
            || self.edit_form.is_some()
            || self.compare_form.is_some()
            || self.browser.is_some()
            || self.auth.prompt.is_some()
    }

    fn route_event(&mut self, event: Event) {
        if self.dialogs.is_empty() && self.auth.prompt.is_none() {
            if let Event::Paste(text) = &event
                && self.ai_paste(text)
            {
                return;
            }
            if let Event::Mouse(mouse) = &event
                && self.on_ai_mouse(*mouse)
            {
                return;
            }
        }
        let blocked = self.modal_is_open();
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) if self.dialogs.is_empty() && self.auth.prompt.is_some() => {
                if let Some(prompt) = &mut self.auth.prompt {
                    prompt.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.rename.is_some() => {
                if let Some(prompt) = &mut self.rename {
                    prompt.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.browser.is_some() => {
                if let Some(browser) = &mut self.browser {
                    browser.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.settings_form.is_some() => {
                if let Some(form) = &mut self.settings_form {
                    form.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.export_form.is_some() => {
                if let Some(form) = &mut self.export_form {
                    form.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.edit_form.is_some() => {
                if let Some(crate::data_edit::form::EditForm {
                    body: crate::data_edit::form::Body::Cell { editor, .. },
                    ..
                }) = &mut self.edit_form
                {
                    editor.on_paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.import_form.is_some() => {
                if let Some(form) = &mut self.import_form {
                    form.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.compare_form.is_some() => {
                if let Some(form) = &mut self.compare_form {
                    form.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.palette.is_some() => {
                if let Some(palette) = &mut self.palette {
                    palette.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.history.is_some() => {
                if let Some(list) = &mut self.history {
                    list.paste(&text);
                }
            }
            Event::Paste(text) if self.dialogs.is_empty() && self.help.is_some() => {
                if let Some(help) = &mut self.help {
                    help.paste(&text);
                }
            }
            Event::Paste(text) if !blocked => self.on_paste(&text),
            Event::Mouse(mouse) if !self.dialogs.is_empty() => self.on_dialog_mouse(mouse),
            Event::Mouse(mouse) if self.auth.prompt.is_some() => self.on_password_mouse(mouse),
            Event::Mouse(mouse) if self.conflicts.is_some() => self.on_conflicts_mouse(mouse),
            Event::Mouse(mouse) if self.browser.is_some() => self.on_browser_mouse(mouse),
            Event::Mouse(mouse) if self.picker.is_some() => self.on_picker_mouse(mouse),
            Event::Mouse(mouse) if self.palette.is_some() => self.on_palette_mouse(mouse),
            Event::Mouse(mouse) if self.history.is_some() => self.on_history_mouse(mouse),
            Event::Mouse(mouse) if self.settings_form.is_some() => self.on_settings_mouse(mouse),
            Event::Mouse(mouse) if self.export_form.is_some() => self.on_export_mouse(mouse),
            Event::Mouse(mouse) if self.edit_form.is_some() => self.on_edit_mouse(mouse),
            Event::Mouse(mouse) if self.import_form.is_some() => self.on_import_mouse(mouse),
            Event::Mouse(mouse) if self.compare_form.is_some() => self.on_compare_mouse(mouse),
            Event::Mouse(mouse) if self.overlay.is_some() => {
                if let Some(overlay) = &mut self.overlay {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            overlay.scroll = overlay.scroll.saturating_sub(3)
                        }
                        MouseEventKind::ScrollDown => {
                            overlay.scroll = overlay.scroll.saturating_add(3)
                        }
                        _ => {}
                    }
                }
            }
            Event::Mouse(mouse) if self.help.is_some() => {
                if let Some(help) = &mut self.help {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => help.scroll = help.scroll.saturating_sub(3),
                        MouseEventKind::ScrollDown => help.scroll = help.scroll.saturating_add(3),
                        _ => {}
                    }
                }
            }
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
            if self.navigator.filter_is_editing() {
                self.navigator.paste_in_filter(text, &self.connections);
            }
            return;
        }
        // A diagram and the home screen have no text field.
        if self.diagram_active.is_some() || self.home() {
            return;
        }
        if let Some(tab) = self.compare_mut() {
            tab.result.paste_in_search(text);
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
        if console.focus == Focus::Parameters {
            console.parameters.paste(text);
            return;
        }
        if console.paste_in_search(text) {
            return;
        }
        if self.agent_tab_without_sql(self.console().id) {
            self.status = Some(text::AGENT_NO_EDITOR.into());
            return;
        }
        if self.review_paste() {
            return;
        }
        let console = self.console_mut();
        console.focus = Focus::Editor;
        console.editor.on_paste(text);
    }

    /// Returns true when the next step must not wait for a key.
    /// Returns true once after a long run ends. The main loop rings the bell, so a
    /// test reads the request and writes nothing to the terminal.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    pub fn has_work(&self) -> bool {
        let search = match (self.compare(), self.table()) {
            (Some(tab), _) => tab.result.search_has_work(),
            (None, Some(tab)) => tab.result.search_has_work(),
            (None, None) => self.console().search_has_work(),
        };
        search || self.exports_have_work() || self.analysis_job.is_some()
    }

    /// Returns the time that the main loop can wait for a key. Events that arrive now
    /// can fill the bounded channel in less time, and a full channel stops the database.
    pub fn key_wait(&self) -> Duration {
        if self.events_flow || self.has_work() {
            Duration::ZERO
        } else {
            KEY_WAIT
        }
    }

    /// Reads waiting execution events and continues the row search.
    /// It returns true when the screen changed.
    pub fn drain_events(&mut self) -> bool {
        let ai_changed = self.poll_ai();
        let agent_changed = self.drain_agent();
        self.continue_agent_jobs();
        self.sync_command_history();
        let ticked = self.tick();
        let authorized = self.poll_auth();
        let probed = self.poll_probes();
        let changed =
            self.has_work() || ticked || authorized || probed || agent_changed || ai_changed;
        if changed {
            let deadline = Instant::now() + WORK_SLICE;
            match (self.compare_active, self.table_active) {
                (Some(index), _) => self.compares[index].result.continue_search(deadline),
                (None, Some(index)) => self.tables[index].result.continue_search(deadline),
                (None, None) => self.console_mut().continue_search(deadline),
            }
            self.continue_exports(deadline);
            self.continue_analysis(deadline);
        }
        // The number of frames must not grow with the events, because a frame has a cost.
        let deadline = Instant::now() + WORK_SLICE;
        self.events_flow = false;
        while let Some(event) = self
            .events
            .as_mut()
            .and_then(|events| events.try_recv().ok())
        {
            self.on_exec_event(event);
            self.events_flow = true;
            if Instant::now() >= deadline {
                break;
            }
        }
        self.poll_agent_jobs();
        self.flush_history();
        changed || self.events_flow
    }

    fn on_exec_event(&mut self, event: ExecEvent) {
        self.note_agent_event(&event);
        match event {
            ExecEvent::AgentMetadata { job, result } => self.on_agent_metadata(&job, result),
            ExecEvent::CommandLog { target, event } => {
                if let Some(console) = self
                    .consoles
                    .iter_mut()
                    .find(|console| console.messages.id == target)
                {
                    console.messages.on_event(event);
                }
            }
            ExecEvent::CatalogLoaded {
                connection,
                read,
                catalog,
            } => {
                self.on_catalog_loaded(connection, read, catalog);
            }
            ExecEvent::ColumnsLoaded {
                connection,
                relation,
                columns,
                foreign_keys,
                referencing,
            } => {
                self.on_columns_loaded(connection, &relation, columns, (foreign_keys, referencing));
            }
            ExecEvent::ConnectionTested { test, result } => self.on_connection_tested(test, result),
            ExecEvent::Disconnected { connection } => self.on_disconnected(connection),
            ExecEvent::IdleClosed { connection } => self.on_idle_closed(connection),
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

    fn on_console_event(&mut self, mut event: ExecEvent) {
        if self.on_edit_event(&mut event)
            || self.on_export_event(&event)
            || self.on_import_event(&mut event)
            || self.on_definition_event(&mut event)
        {
            return;
        }
        let Some(id) = event.console() else { return };
        let budget = self.budget.clone();
        if let Some(tab) = self.compares.iter_mut().find(|tab| tab.owns(id)) {
            tab.on_event(event, &budget);
            self.continue_compare(id);
            return;
        }
        if let Some(tab) = self.diagrams.iter_mut().find(|tab| tab.id == id) {
            tab.on_event(event, &budget);
            return;
        }
        if let Some(index) = self.tables.iter().position(|tab| tab.id == id) {
            let finished = matches!(event, ExecEvent::ExecutionFinished { .. });
            if finished {
                let auto = self.tables[index].auto_execution;
                self.on_auto_execution_end(&event, id, auto);
            }
            self.tables[index].on_event(event, &budget);
            if finished
                && !self.tables[index].is_busy()
                && let Some(restore) = self.tables[index].edit_restore.take()
                && let Some(view) = restore.view_in(&self.tables[index].result.table)
            {
                self.tables[index].result.view = view;
            }
            self.finish_session_signal(index);
            self.continue_wanted_import();
            return;
        }
        if let ExecEvent::StatementFinished { key, status, .. } = &event
            && self
                .console_index(id)
                .is_none_or(|index| self.consoles[index].auto_execution != Some(key.execution))
        {
            self.note_statement(key, status);
        }
        if let Some(index) = self.console_index(id) {
            let auto = self.consoles[index].auto_execution;
            if matches!(event, ExecEvent::ExecutionFinished { .. }) {
                self.on_auto_execution_end(&event, id, auto);
            }
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
        self.choose_theme();
        let area = frame.area();
        self.layout.agent_bar = Default::default();
        self.layout.agent_card = Default::default();
        self.layout.agent_notice = None;
        frame.render_widget(Block::new().style(self.theme.base), area);
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            draw_small_terminal(frame, area, (MIN_WIDTH, MIN_HEIGHT), &self.theme);
            return;
        }
        let tall = area.height >= TALL_HEIGHT;
        let agent_bar = self.agent_bar();
        let [tabs, rule, body, notice, status, hints] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(u16::from(tall)),
            Constraint::Min(0),
            Constraint::Length(u16::from(agent_bar.is_some())),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        self.layout.body = body;
        let views = self.tab_views();
        (self.layout.main_tabs, self.layout.quit) =
            draw_main_tabs(frame, tabs, views, self.active_tab(), &self.theme);
        if tall {
            draw_rule(frame, rule, &self.theme);
        }
        let main = self.draw_side(frame, body);
        if self.compare_active.is_some() {
            self.draw_active_compare(frame, main, tall);
        } else if self.diagram_active.is_some() {
            self.draw_active_diagram(frame, main, tall);
        } else if self.table_active.is_some() {
            self.draw_active_table(frame, main, tall);
        } else if self.home() {
            self.draw_home(frame, main);
        } else {
            self.draw_active_console(frame, main, tall);
        }
        if let Some(bar) = agent_bar {
            self.layout.agent_bar = crate::ui::agent::draw_bar(frame, notice, &bar, &self.theme);
        }
        if let Some((notice, card)) = self.agent_card() {
            self.layout.agent_card = crate::ui::agent::draw_card(frame, main, &card, &self.theme);
            self.layout.agent_notice = Some(notice);
        }
        self.draw_status(frame, status);
        self.draw_hint(frame, hints);
        self.draw_layers(
            frame,
            Rect {
                y: 0,
                height: notice.y,
                ..area
            },
        );
    }

    /// Draws the navigator and its divider. It returns the area for the console.
    fn draw_side(&mut self, frame: &mut Frame, body: Rect) -> Rect {
        if self.navigator.collapsed {
            self.layout.navigator_title = Rect::default();
            self.layout.navigator_filter = Rect::default();
            self.layout.navigator = Rect::default();
            self.layout.divider = None;
            return body;
        }
        let width = self
            .navigator
            .width
            .min(body.width.saturating_sub(mouse::MIN_MAIN_WIDTH));
        let [side, divider, main] = Layout::horizontal([
            Constraint::Length(width),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(body);
        let [title, tree] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(side);
        let focused = self.navigator_focused;
        let connected: Vec<bool> = self
            .connections
            .iter()
            .map(|connection| {
                self.manager
                    .as_ref()
                    .is_some_and(|manager| manager.is_connected(connection.id))
            })
            .collect();
        frame.buffer_mut().set_style(side, self.theme.panel);
        draw_panel_title(frame, title, text::TITLE_EXPLORER, focused, &self.theme);
        let (filter, rows) = draw_navigator(
            frame,
            tree,
            &mut self.navigator,
            &self.connections,
            &connected,
            focused,
            &self.theme,
        );
        let line = vec![Line::raw("│"); usize::from(divider.height)];
        frame.render_widget(Paragraph::new(line).style(self.theme.divider), divider);
        self.layout.navigator_title = title;
        self.layout.navigator_filter = filter;
        self.layout.navigator = rows;
        self.layout.divider = Some(divider.x);
        main
    }

    fn draw_active_console(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let connection = self.active_connection();
        let name = connection
            .map_or(text::NO_DATABASE_LABEL, |connection| &connection.name)
            .to_owned();
        let namespace = connection.map_or(Namespace::None, |item| item.capabilities.namespace);
        let transactions = connection.is_none_or(|item| item.capabilities.transactions);
        let default_schema = connection.and_then(|item| item.default_schema.clone());
        let parameters_shortcut = self
            .keymap
            .keys(crate::input::Action::FocusParameters)
            .join(", ");
        let bar = BarData {
            parameters_shortcut: &parameters_shortcut,
            connection: &name,
            namespace,
            default_schema: default_schema.as_deref(),
            transactions,
            title: tall,
            welcome: self.welcome(),
            show_system_commands: self.show_system_commands,
            agent: self.agent_tab_without_sql(self.consoles[self.active].id),
            agent_sql: self.agent.owns(self.consoles[self.active].id),
            agent_access: self.agent.tab_access(self.consoles[self.active].id),
            agent_tasks: self.agent.tab_tasks(self.consoles[self.active].id),
        };
        let ratio = self.editor_ratio;
        let focused = !self.navigator_focused;
        let console = &mut self.consoles[self.active];
        self.layout.console = draw_console(frame, area, console, &bar, ratio, focused, &self.theme);
    }

    /// Draws the screen without a tab. The console areas stay empty, so the mouse finds
    /// no editor.
    fn draw_home(&mut self, frame: &mut Frame, area: Rect) {
        self.layout.console = ConsoleLayout::default();
        draw_welcome(frame, area, &self.home_screen(), &self.theme);
    }

    fn draw_active_table(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let Some(index) = self.table_active else {
            return;
        };
        let connection = self.tables[index].connection;
        let name = self
            .connection(connection)
            .map_or_else(String::new, |item| item.name.clone());
        let bar = TableBar {
            connection: &name,
            blocked: self.tables[index].is_blocked(&self.budget),
            title: tall,
        };
        let focused = !self.navigator_focused;
        self.layout.table = draw_table(
            frame,
            area,
            &mut self.tables[index],
            &bar,
            focused,
            &self.theme,
        );
    }

    fn draw_active_compare(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let Some(index) = self.compare_active else {
            return;
        };
        let focused = !self.navigator_focused;
        self.layout.compare = draw_compare(
            frame,
            area,
            &mut self.compares[index],
            tall,
            focused,
            &self.theme,
        );
    }

    fn draw_active_diagram(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let Some(index) = self.diagram_active else {
            return;
        };
        let connection = self.diagrams[index].connection;
        let name = self
            .connection(connection)
            .map_or_else(String::new, |item| item.name.clone());
        let bar = DiagramBar {
            connection: &name,
            title: tall,
        };
        let focused = !self.navigator_focused;
        self.layout.diagram = draw_diagram(
            frame,
            area,
            &mut self.diagrams[index],
            &bar,
            focused,
            &self.theme,
        );
    }

    /// Returns the name of the place that receives keys, for the status line.
    fn status_panel(&self) -> StatusPanel {
        if self.navigator_focused {
            return if self.navigator.filter_is_editing() {
                StatusPanel::Filter
            } else {
                StatusPanel::Explorer
            };
        }
        if self.diagram_active.is_some() {
            return StatusPanel::Diagram;
        }
        if self.compare_active.is_some() {
            return StatusPanel::Table;
        }
        if self.home() {
            return StatusPanel::Home;
        }
        if let Some(tab) = self.table() {
            return if tab.focus == TableFocus::Grid {
                StatusPanel::Table
            } else {
                StatusPanel::Filter
            };
        }
        let console = self.console();
        if console.focus == Focus::Results {
            return StatusPanel::Results;
        }
        if console.focus == Focus::Parameters {
            return StatusPanel::Filter;
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

    /// Writes the effective key of an agent command, or its path in the palette.
    pub(super) fn agent_shortcut(
        &self,
        action: crate::input::Action,
        name: &str,
        entry: &str,
    ) -> Option<String> {
        self.shown_key(action)
            .map(|key| format!("{key}: {name}"))
            .or_else(|| {
                self.shown_key(crate::input::Action::OpenPalette)
                    .map(|key| format!("{key} > {entry}"))
            })
    }

    /// Returns the MCP row while an item waits for the user or an agent task runs.
    fn agent_bar(&self) -> Option<crate::ui::agent::Bar> {
        let access = self.agent.pending_access();
        let proposals = self.agent.pending_proposals();
        let tasks = self.agent.active_tasks();
        if access + proposals + tasks == 0 {
            return None;
        }
        let (action, name, entry) = if access != 0 {
            (
                crate::input::Action::AgentSessions,
                "sessions",
                "Agent sessions",
            )
        } else {
            (
                crate::input::Action::AgentReview,
                "review",
                "Review agent changes",
            )
        };
        let shortcut = (access + proposals != 0).then(|| self.agent_shortcut(action, name, entry));
        Some(crate::ui::agent::Bar {
            items: text::agent_bar(access, proposals, tasks),
            shortcut: shortcut.flatten(),
            review: access + proposals != 0,
            stop: tasks != 0,
        })
    }

    fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let panel = self.status_panel();
        let data = (self.budget.used(), self.budget.limit());
        let agent = self.agent.status_label();
        let agent = agent.as_deref();
        if let Some(tab) = self.compare() {
            let view = tab.result.view;
            let position =
                (!tab.result.table.rows.is_empty()).then_some((view.row + 1, view.col + 1));
            let target = &tab.sides[1].side;
            let item = self.connection(target.connection);
            let status = StatusData {
                panel,
                connection: item.map_or("", |item| item.name.as_str()),
                production: item.is_some_and(Connection::is_production),
                schema: Some(&target.schema),
                session: None,
                transaction_age: None,
                phase: tab.phase,
                run_time: run_time(tab.phase, &tab.run, self.clock),
                pool: self
                    .manager
                    .as_ref()
                    .map(|manager| manager.usage(target.connection)),
                data,
                position,
                agent,
                auto: None,
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        if let Some(tab) = self.diagram() {
            let item = self.connection(tab.connection);
            let status = StatusData {
                panel,
                connection: item.map_or("", |item| item.name.as_str()),
                production: item.is_some_and(Connection::is_production),
                schema: Some(tab.focus.schema()),
                session: None,
                transaction_age: None,
                phase: tab.phase,
                run_time: run_time(tab.phase, &tab.run, self.clock),
                pool: self
                    .manager
                    .as_ref()
                    .map(|manager| manager.usage(tab.connection)),
                data,
                position: None,
                agent,
                auto: None,
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        if let Some(tab) = self.table() {
            let view = tab.result.view;
            let position =
                (!tab.result.table.rows.is_empty()).then_some((view.row + 1, view.col + 1));
            let item = self.connection(tab.connection);
            let status = StatusData {
                panel,
                connection: item.map_or("", |item| item.name.as_str()),
                production: item.is_some_and(Connection::is_production),
                schema: Some(&tab.relation.schema),
                session: None,
                transaction_age: None,
                phase: tab.phase,
                run_time: run_time(tab.phase, &tab.run, self.clock),
                pool: self
                    .manager
                    .as_ref()
                    .map(|manager| manager.usage(tab.connection)),
                data,
                position,
                agent,
                auto: tab.auto_refresh.map(|auto| auto.every.as_secs()),
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        if self.home() {
            let status = StatusData {
                panel,
                connection: "",
                production: false,
                schema: None,
                session: None,
                transaction_age: None,
                phase: Phase::Idle,
                run_time: None,
                pool: None,
                data,
                position: None,
                agent,
                auto: None,
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        let console = self.console();
        let connection = self.active_connection();
        let namespace = connection.map_or(Namespace::None, |item| item.capabilities.namespace);
        let default_schema = connection.and_then(|item| item.default_schema.as_deref());
        let result = console
            .results
            .get(console.active)
            .filter(|_| console.focus == Focus::Results);
        let position = match result {
            Some(result) => (!result.table.rows.is_empty())
                .then_some((result.view.row + 1, result.view.col + 1)),
            None => {
                let cursor = console.editor.cursor();
                Some((cursor.row + 1, cursor.col + 1))
            }
        };
        let status = StatusData {
            panel,
            connection: connection.map_or(text::NO_DATABASE_LABEL, |item| item.name.as_str()),
            production: connection.is_some_and(Connection::is_production),
            schema: (namespace != Namespace::None)
                .then(|| console.schema.as_deref().or(default_schema))
                .flatten(),
            session: Some((console.manual, console.transaction)),
            transaction_age: console.transaction_age(self.clock),
            phase: console.phase,
            run_time: run_time(console.phase, &console.run, self.clock),
            pool: console
                .connection
                .zip(self.manager.as_ref())
                .map(|(id, manager)| manager.usage(id)),
            data,
            position,
            agent,
            auto: console.auto_refresh.map(|auto| auto.every.as_secs()),
        };
        draw_statusline(frame, area, &status, &self.theme);
    }

    fn draw_hint(&self, frame: &mut Frame, area: Rect) {
        let message = self
            .status
            .clone()
            .or_else(|| self.export_progress())
            .or_else(|| self.import_progress())
            .or_else(|| self.auth_wait().map(str::to_owned));
        // The row starts one column after the edge, as the status line does.
        let area = Rect {
            x: area.x + 1,
            width: area.width.saturating_sub(1),
            ..area
        };
        let line = match message {
            Some(status) => Line::styled(status, self.theme.status),
            None => {
                let items = self.fitted_hints(usize::from(area.width));
                hint_line(&items, &self.theme)
            }
        };
        frame.render_widget(Paragraph::new(line), area);
    }

    /// Draws the windows above the panels. `bounds` holds the rows above the status line.
    fn draw_layers(&mut self, frame: &mut Frame, bounds: Rect) {
        self.draw_ai(frame);
        if self.command_page.is_some_and(|region| {
            region != Region::Navigator && self.context_region() != Some(region)
        }) {
            self.command_page = None;
        }
        if let Some(region) = self.command_page {
            let (title, menu) = if region == Region::Navigator {
                (self.explorer_menu_title(), self.explorer_menu())
            } else {
                (self.context_menu_title(), self.context_menu())
            };
            draw_which_key(frame, bounds, &title, &menu, &self.theme);
        } else if self.pending_prefix() {
            let title = text::TITLE_PREFIX;
            draw_which_key(frame, bounds, title, &self.prefix_menu(), &self.theme);
        }
        if let Some(form) = &self.form {
            self.layout.form = draw_form(frame, form, &self.theme);
        }
        if let Some(picker) = &self.picker {
            let items: Vec<&str> = picker.items.iter().map(String::as_str).collect();
            self.layout.picker = draw_picker(
                frame,
                &picker.title,
                &items,
                picker.cursor,
                picker.loading,
                &self.theme,
            );
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
            let Overlay {
                title,
                contents,
                footer,
                scroll,
                ..
            } = overlay;
            *scroll = draw_overlay(frame, title, contents, footer, *scroll, &self.theme);
        }
        if let Some(help) = &mut self.help {
            draw_help(frame, help, &self.theme);
        }
        if let Some(palette) = &self.palette {
            self.layout.palette = draw_palette(frame, palette, &self.theme);
        }
        if let Some(list) = &self.history {
            self.layout.history = draw_history(frame, list, &self.theme);
        }
        if let Some(form) = &self.settings_form {
            self.layout.settings = draw_settings(frame, form, &self.theme);
        }
        if let Some(form) = &self.export_form {
            self.layout.export = draw_export(frame, form, &self.theme);
        }
        if let Some(form) = &mut self.edit_form {
            self.layout.edit = crate::ui::data_edit::draw_edit(frame, form, &self.theme);
        }
        if let Some(form) = &self.import_form {
            self.layout.import = draw_import(frame, form, &self.theme);
        }
        if let Some(form) = &self.compare_form {
            self.layout.compare_form = draw_compare_form(frame, form, &self.theme);
        }
        if let Some(browser) = &self.browser {
            self.layout.browser = draw_browser(frame, browser, &self.theme);
        }
        if let Some(prompt) = &self.rename {
            draw_name(frame, prompt, &self.theme);
        }
        if let Some(prompt) = &self.auth.prompt {
            self.layout.password = crate::ui::auth::draw_password(frame, prompt, &self.theme);
        }
        self.draw_ai_prompts(frame);
        if let Some(dialog) = self.dialogs.front() {
            let labels: Vec<&str> = dialog.options.iter().map(|(label, _)| *label).collect();
            self.layout.dialog = draw_dialog(
                frame,
                &dialog.title,
                &dialog.body,
                &labels,
                dialog.focus,
                &self.theme,
            );
        }
    }
}

pub fn run(options: Options) -> io::Result<()> {
    let clipboard: SharedClipboard = Rc::new(RefCell::new(SystemClipboard::new()));
    let report_folder = options.store.as_ref().map(|store| store.global.dir.clone());
    // The start of the terminal can wait for an answer, and a signal can arrive meanwhile.
    let signal = signal_flag()?;
    // The watchdog starts before the session, because a query of the terminal can
    // also spin when the terminal closes during the start.
    watch_terminal();
    let mut session = Session::start(report_folder)?;
    let mut app = App::new(options, clipboard, Theme::from_env());
    let result = catch_unwind(AssertUnwindSafe(|| {
        event_loop(&mut app, &mut session, &signal)
    }));
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
        if poll(app.key_wait())? {
            app.on_event(read()?);
            while poll(Duration::ZERO)? {
                app.on_event(read()?);
            }
            dirty = true;
        }
        dirty |= app.drain_events();
        if app.take_bell() {
            ring_bell();
        }
    }
}
