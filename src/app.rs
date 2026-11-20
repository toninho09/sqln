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
mod draw;
mod events;
mod export;
mod folders;
mod guard;
mod hints;
mod history;
mod import;
mod keys;
mod main_loop;
pub use main_loop::run;
mod modal;
use modal::Modals;
mod mouse;
mod persist;
mod review;
mod scripts;
mod session;
mod sql_file;
mod tables;
use tables::{Screen, Tabs};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use tokio::sync::mpsc::Receiver;

use self::keys::RunTarget;
use self::mouse::Drag;
use crate::catalog::{Catalog, RelationRef};
use crate::clipboard::SharedClipboard;
use crate::connection::{Connection, Definition};
use crate::connector::{Connector, TransactionCommand};
use crate::console::{AfterTransaction, Console};
use crate::creds::{Identity, PlainFile};
use crate::definition::Target;
use crate::editor::{DEFAULT_UNDO_LIMIT, KeyboardMode, SqlEditor};
use crate::exec::{ConnectionId, ConsoleId, ExecEvent, Manager, Settings};
use crate::export::ExportChoices;
use crate::form::ConnectionForm;
use crate::generate::Template;
use crate::grid::Budget;
use crate::import::ImportChoices;
use crate::input::{Chord, Keymap, Region, SuggestionKeys};
use crate::navigator::Navigator;
use crate::sql::Dialect;
use crate::store::config::{ExplorerSide, IconName, ThemeName};
use crate::store::scripts::ScriptEntry;
use crate::store::{Scope, Store};
use crate::structure::SessionSignal;
use crate::table::Follow;
use crate::ui::compare::CompareLayout;
use crate::ui::export::ExportLayout;
use crate::ui::form::FormLayout;
use crate::ui::import::ImportLayout;
use crate::ui::settings::SettingsLayout;
use crate::ui::theme::{ColorEnv, Theme, ThemeCatalog};
use crate::ui::wrap::WrappedText;
use crate::ui::{ConflictLayout, ConsoleLayout, MainTab, TableLayout, text};

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
    pub explorer_side: ExplorerSide,
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
            explorer_side: ExplorerSide::default(),
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
    /// Removes a folder of the explorer. Its connections stay.
    Ungroup(crate::store::config::FolderRef),
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
    _ai_memory: Option<crate::ai::Memory>,
    title: String,
    contents: WrappedText,
    footer: String,
    scroll: usize,
    dragging: bool,
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
            self.contents.highlight_json();
            self.other = Some((shown, old_footer));
            self.scroll = 0;
            self.dragging = false;
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
    /// The folders of the scope of a connection, with a new folder first.
    Folder {
        connection: ConnectionId,
        choices: Vec<folders::FolderChoice>,
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
    overlay: Rect,
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
    modals: Modals,
    tabs: Tabs,
    appearance: Appearance,
    time: Clock,
    input: InputState,
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
    explorer_side: ExplorerSide,
    show_system_commands: bool,
    database: Option<PathBuf>,
    clipboard: SharedClipboard,
    /// The theme of the current frame. `choose_theme` writes it at the start of each
    /// frame, so a change of tab or of a choice needs no other step.
    theme: Theme,
    layout: ScreenLayout,
    editor_ratio: u16,
    analysis_job: Option<crate::analysis::Job>,
    status: Option<String>,
    /// The message on the clock, with the time of its first frame.
    status_shown: Option<(String, Instant)>,
    /// True when a long run ended and the main loop did not ring the bell yet.
    bell: bool,
    /// True when the last read found execution events.
    events_flow: bool,
    /// A SQL template that waits for the columns of its relation.
    pending_sql: Option<(SqlRequest, Template)>,
    /// The connections whose diagrams wait for their catalogs, which give the current
    /// schemas.
    pending_diagrams: Vec<ConnectionId>,
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
    /// The answers of the vault writes of Schema Registry passwords.
    registry_writes: connections::VaultWrites,
    /// A table tab that opens the import dialog after its first page.
    import_wanted: Option<ConsoleId>,
    /// The files of the application. It is `None` when nothing loads or saves.
    persist: Option<persist::Persist>,
    auth: auth::Auth,
    agent: agent::State,
    /// The SQL rules of a console that has no connection.
    fallback_dialect: Dialect,
    next_console_number: usize,
    next_unbound_console: ConsoleId,
    next_test: u64,
    exiting: bool,
    quit: bool,
}

/// The theme sources: the terminal, the environment, the choices of the files, and the custom themes.
struct Appearance {
    /// The theme of the terminal, before the choices of the configuration.
    terminal_theme: Theme,
    color_env: ColorEnv,
    /// The `theme` and `icons` choices of the files.
    choices: (Option<ThemeName>, Option<IconName>),
    /// The custom themes of both scopes, after the precedence.
    custom_themes: ThemeCatalog,
}

/// The times of the main loop.
struct Clock {
    /// The start of the application, for the pictures of the mark of a run.
    started: Instant,
    /// The time of the last step of the main loop. The screen uses it for each run.
    now: Instant,
    run_frame: usize,
    /// The whole minutes of the open transaction on screen in the last frame.
    transaction_minute: Option<u64>,
}

/// The state of a key sequence, a command page, and the mouse.
struct InputState {
    /// The region and keys of a sequence that needs more keys.
    pending_key: Option<(Region, Vec<Chord>)>,
    /// The region of the command page that waits for its next key.
    command_page: Option<Region>,
    drag: Option<Drag>,
    /// The time and the place of the last click in the navigator.
    last_click: Option<(Instant, u16, u16)>,
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
            (None, Some(definition), None) => crate::connector::open(definition).dialect(),
            (None, None, None) => stored.first().map_or(options.dialect, |item| {
                crate::connector::open(&item.definition).dialect()
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
            explorer_side: options.explorer_side,
            ai: ai::State::new(options.ai_completion, options.ai_vault),
            modals: Modals::default(),
            tabs: Tabs::default(),
            input: InputState {
                pending_key: None,
                command_page: None,
                drag: None,
                last_click: None,
            },
            time: Clock {
                started: Instant::now(),
                now: Instant::now(),
                run_frame: 0,
                transaction_minute: None,
            },
            appearance: Appearance {
                terminal_theme: theme,
                color_env,
                choices: options.appearance,
                custom_themes: options.custom_themes.clone(),
            },
            show_system_commands: options.show_system_commands,
            database: options.database,
            clipboard,
            theme,
            layout: ScreenLayout::default(),
            editor_ratio: DEFAULT_EDITOR_RATIO,
            analysis_job: None,
            sql_file_open: None,
            sql_file: None,
            export_choices: options.export_choices.clone(),
            exports: Vec::new(),
            import_choices: options.import_choices.clone(),
            imports: Vec::new(),
            definitions: Vec::new(),
            pending_follow: None,
            import_probes: import::ProbeChannel::default(),
            registry_writes: connections::VaultWrites::default(),
            import_wanted: None,
            status: None,
            status_shown: None,
            bell: false,
            events_flow: false,
            pending_sql: None,
            pending_diagrams: Vec::new(),
            persist,
            auth: auth::Auth::new(options.vault.take(), plain),
            agent: agent::State::default(),
            fallback_dialect: options.dialect,
            next_console_number: 1,
            next_unbound_console: FIRST_UNBOUND_CONSOLE,
            next_test: 1,
            exiting: false,
            quit: false,
        };
        let console = app.unbound_console(dialect);
        let first = console.id;
        app.tabs.consoles.push(console);
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
            app.tabs.consoles[index].hidden = true;
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
        if let Some(ThemeName::Custom(id)) = &self.appearance.choices.0
            && self.appearance.custom_themes.get(id).is_none()
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
        self.tabs.next_place()
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
        &self.tabs.consoles[self.tabs.active]
    }

    pub fn console_mut(&mut self) -> &mut Console {
        &mut self.tabs.consoles[self.tabs.active]
    }

    pub fn consoles(&self) -> &[Console] {
        &self.tabs.consoles
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
        self.modals.form.as_ref()
    }

    fn console_index(&self, id: ConsoleId) -> Option<usize> {
        self.tabs
            .consoles
            .iter()
            .position(|console| console.id == id)
    }

    fn console_by_id(&mut self, id: ConsoleId) -> Option<&mut Console> {
        self.tabs
            .consoles
            .iter_mut()
            .find(|console| console.id == id)
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
        if let Some(form) = &self.modals.settings_form {
            return form.preview_theme();
        }
        let chosen = self
            .modals
            .form
            .as_ref()
            .and_then(|form| form.theme.clone());
        let connection = || self.tab_connection()?.definition.as_ref()?.theme.clone();
        chosen
            .or_else(connection)
            .or_else(|| self.appearance.choices.0.clone())
    }

    /// The palette of a custom id: the dialog of the settings comes first, because
    /// its drafts preview on the screen.
    fn draft_scheme(&self, id: &str) -> Option<crate::ui::schemes::Scheme> {
        if let Some(form) = &self.modals.settings_form {
            if let Some(scheme) = form.theme_scheme(id) {
                return Some(scheme);
            }
            if form.theme_gone(id) {
                return None;
            }
        }
        self.appearance.custom_themes.scheme(id)
    }

    fn choose_theme(&mut self) {
        let name = self.theme_choice();
        let base = self.appearance.terminal_theme;
        let (_, icons) = self.appearance.choices;
        let env = self.appearance.color_env;
        self.theme = base.with_choices(name, |id| self.draft_scheme(id), icons, env);
    }

    /// Adds a question. It opens now, or after the questions before it.
    fn show_dialog(
        &mut self,
        title: impl Into<String>,
        body: String,
        options: Vec<(&'static str, DialogAction)>,
    ) {
        self.modals.dialogs.push_back(Dialog {
            title: title.into(),
            body,
            options,
            focus: 0,
        });
    }

    fn sync_command_history(&mut self) {
        let active = self
            .console_on_screen()
            .then(|| self.tabs.consoles[self.tabs.active].id);
        let Some(manager) = &mut self.manager else {
            return;
        };
        for console in &self.tabs.consoles {
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
}
