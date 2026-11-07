//! The settings dialog. It edits the preferences of one scope and writes nothing
//! before the save.

use crossterm::event::{KeyCode, KeyEvent};

use crate::ai::{Credential, Provider, Reasoning};
use crate::editor::DEFAULT_UNDO_LIMIT;
use crate::input::SuggestionKeys;

pub mod shortcuts;
use crate::line_input::LineInput;
use crate::store::Scope;
use crate::store::config::{
    AgentAccess, AgentDefault, DEFAULT_BELL_AFTER_SECONDS, DEFAULT_CAP_IN_SQL, DEFAULT_FORMAT_JSON,
    DEFAULT_MCP, DEFAULT_MCP_REMEMBER_CLIENTS, DEFAULT_QUERY_HISTORY, DEFAULT_REOPEN_TABS,
    DEFAULT_SHOW_SYSTEM_COMMANDS, KeyboardName, Keymaps, Preferences, ThemeName,
};
use crate::ui::text;
use shortcuts::Shortcuts;

const MEBIBYTE: usize = 1024 * 1024;
/// The defaults of `Settings`, for the note of a field without a value.
pub const DEFAULT_KEYBOARD: KeyboardName = KeyboardName::Conventional;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// The sidebar. It is one place of the focus.
    Sections,
    Scope,
    Keyboard,
    PageSize,
    RowCap,
    CapInSql,
    Budget,
    ReopenTabs,
    QueryHistory,
    BellAfter,
    FormatJson,
    ShowSystemCommands,
    SuggestionKeys,
    UndoLimit,
    Theme,
    Mcp,
    McpDefault,
    McpMax,
    McpRemember,
    AiEnabled,
    AiProfile,
    AiModel,
    AiEndpoint,
    AiSource,
    AiPath,
    AiKey,
    AiContext,
    AiOutput,
    AiTimeout,
    AiReasoning,
    AiSimultaneous,
    AiTest,
    ShortcutMode,
    ShortcutCommands,
    ShortcutBack,
    ShortcutBinding,
    ShortcutFinish,
    ShortcutRecord,
    ShortcutRemove,
    ShortcutRestore,
    ShortcutOk,
    ShortcutCancel,
    Save,
    Cancel,
}

/// One group of the sidebar.
pub struct Section {
    pub title: &'static str,
    pub fields: &'static [Field],
    /// Text below the fields. It takes no focus.
    pub help: Option<&'static str>,
}

/// The sidebar. To add a group, add a row here.
pub const SECTIONS: [Section; 7] = [
    Section {
        title: text::SECTION_GENERAL,
        fields: &[
            Field::Keyboard,
            Field::ReopenTabs,
            Field::QueryHistory,
            Field::BellAfter,
        ],
        help: None,
    },
    Section {
        title: text::SECTION_EDITOR,
        fields: &[Field::SuggestionKeys, Field::UndoLimit],
        help: None,
    },
    Section {
        title: text::SECTION_RESULTS,
        fields: &[
            Field::PageSize,
            Field::RowCap,
            Field::CapInSql,
            Field::Budget,
            Field::FormatJson,
            Field::ShowSystemCommands,
        ],
        help: None,
    },
    Section {
        title: text::SECTION_APPEARANCE,
        fields: &[Field::Theme],
        help: None,
    },
    Section {
        title: text::SECTION_MCP,
        fields: &[
            Field::Mcp,
            Field::McpDefault,
            Field::McpMax,
            Field::McpRemember,
        ],
        help: Some(text::HELP_MCP),
    },
    Section {
        title: text::SECTION_AUTOCOMPLETE,
        fields: &[
            Field::AiEnabled,
            Field::AiProfile,
            Field::AiModel,
            Field::AiEndpoint,
            Field::AiSource,
            Field::AiPath,
            Field::AiKey,
            Field::AiContext,
            Field::AiOutput,
            Field::AiTimeout,
            Field::AiReasoning,
            Field::AiSimultaneous,
            Field::AiTest,
        ],
        help: None,
    },
    Section {
        title: text::SECTION_SHORTCUTS,
        fields: shortcuts::FIELDS,
        help: None,
    },
];

pub enum SettingsRequest {
    AiTest,
    None,
    Cancel,
    Save,
}

/// The autocomplete fields of one scope. The typed key stays in memory only.
#[derive(Clone)]
struct AiDraft {
    /// True after an edit or when the scope already had a block. A save writes
    /// the fields only then.
    touched: bool,
    saved: Option<crate::ai::Preferences>,
    enabled: Option<bool>,
    inherit: bool,
    provider: Provider,
    model: LineInput,
    endpoint: LineInput,
    path: LineInput,
    key: LineInput,
    context: LineInput,
    output: LineInput,
    timeout: LineInput,
    simultaneous: LineInput,
    source: usize,
    /// The index of the reasoning effort in `Reasoning::ALL`.
    reasoning: usize,
}

impl AiDraft {
    /// Builds the fields of one scope. A scope without its own provider shows the
    /// effective provider and inherits it.
    fn of(
        preferences: Option<&crate::ai::Preferences>,
        effective: &crate::ai::Preferences,
    ) -> Self {
        let provider = preferences
            .and_then(|p| p.provider.clone())
            .or_else(|| effective.provider.clone())
            .unwrap_or_default();
        let source = match &provider.credential {
            Credential::None => 0,
            Credential::Vault { .. } => 1,
            Credential::File { .. } => 2,
        };
        let path = match &provider.credential {
            Credential::File { path } => path.as_str(),
            _ => "",
        };
        let reasoning = Reasoning::ALL
            .iter()
            .position(|value| *value == provider.reasoning_effort)
            .unwrap_or(0);
        Self {
            touched: preferences.is_some(),
            saved: preferences.cloned(),
            enabled: preferences.and_then(|p| p.enabled),
            inherit: preferences.is_none_or(|p| p.provider.is_none()),
            model: input(&provider.model),
            endpoint: input(&provider.base_url),
            path: input(path),
            key: LineInput::default(),
            context: input(&provider.context_window_tokens.to_string()),
            output: input(&provider.max_output_tokens.to_string()),
            timeout: input(&provider.timeout_seconds.to_string()),
            simultaneous: input(&effective.max_simultaneous().to_string()),
            provider,
            source,
            reasoning,
        }
    }
}

fn input(text: &str) -> LineInput {
    let mut line = LineInput::default();
    line.set(text);
    line
}

/// True for every field of the Autocomplete section, except the test button.
fn ai_owned(field: Field) -> bool {
    matches!(
        field,
        Field::AiEnabled
            | Field::AiProfile
            | Field::AiModel
            | Field::AiEndpoint
            | Field::AiSource
            | Field::AiPath
            | Field::AiKey
            | Field::AiContext
            | Field::AiOutput
            | Field::AiTimeout
            | Field::AiReasoning
            | Field::AiSimultaneous
    )
}

/// True while the focus sits on a field that takes any text.
fn ai_text(field: Field) -> bool {
    matches!(
        field,
        Field::AiModel | Field::AiEndpoint | Field::AiPath | Field::AiKey
    )
}

/// The fields of one scope. An empty field removes the definition from the scope.
#[derive(Clone)]
struct Draft {
    keyboard: Option<KeyboardName>,
    page_size: LineInput,
    row_cap: LineInput,
    cap_in_sql: Option<bool>,
    budget: LineInput,
    undo_limit: LineInput,
    bell_after: LineInput,
    reopen_tabs: Option<bool>,
    query_history: Option<bool>,
    format_json: Option<bool>,
    show_system_commands: Option<bool>,
    suggestion_keys: Option<SuggestionKeys>,
    theme: Option<ThemeName>,
    mcp: Option<bool>,
    mcp_default: Option<AgentDefault>,
    mcp_max: Option<AgentAccess>,
    mcp_remember: Option<bool>,
    ai: AiDraft,
    keymap: Option<Keymaps>,
}

/// Returns a size as the whole MiB of a field.
fn mebibytes(bytes: Option<usize>) -> Option<usize> {
    bytes.map(|bytes| bytes.div_ceil(MEBIBYTE))
}

fn number_text<T: ToString>(value: Option<T>) -> LineInput {
    let mut input = LineInput::default();
    input.set(&value.map_or_else(String::new, |value| value.to_string()));
    input
}

impl Draft {
    fn of(preferences: &Preferences, global: &Preferences) -> Self {
        let effective = crate::ai::Preferences::resolve(
            global.ai_completion.as_ref(),
            preferences.ai_completion.as_ref(),
        );
        Self {
            keyboard: preferences.keyboard_mode,
            page_size: number_text(preferences.page_size),
            row_cap: number_text(preferences.row_cap),
            cap_in_sql: preferences.cap_in_sql,
            budget: number_text(mebibytes(preferences.grid_budget_bytes)),
            undo_limit: number_text(mebibytes(preferences.undo_limit_bytes)),
            bell_after: number_text(preferences.bell_after_seconds),
            reopen_tabs: preferences.reopen_tabs,
            query_history: preferences.query_history,
            format_json: preferences.format_json,
            show_system_commands: preferences.show_system_commands,
            suggestion_keys: preferences.suggestion_keys,
            theme: preferences.theme,
            mcp: preferences.mcp,
            mcp_default: preferences.mcp_default_access,
            mcp_max: preferences.mcp_max_access,
            mcp_remember: preferences.mcp_remember_clients,
            ai: AiDraft::of(preferences.ai_completion.as_ref(), &effective),
            keymap: preferences.keymap.clone(),
        }
    }
}

pub struct SettingsForm {
    pub focus: Field,
    /// The index of the open section in `SECTIONS`.
    pub section: usize,
    pub scope: Scope,
    pub note: Option<String>,
    /// True while a vault write or a connection test runs.
    pub ai_busy: bool,
    pub shortcuts: Shortcuts,
    global: Preferences,
    project: Option<Preferences>,
    global_draft: Draft,
    project_draft: Option<Draft>,
}

/// Returns the value after `value` in a list of choices.
pub fn next_choice<T: Copy + PartialEq>(order: &[T], value: T, forward: bool) -> T {
    let index = order.iter().position(|item| *item == value).unwrap_or(0);
    let next = if forward {
        index + 1
    } else {
        index + order.len() - 1
    };
    order[next % order.len()]
}

/// Returns the value after `value` in a list that starts with no value.
pub fn next_optional<T: Copy + PartialEq>(
    values: &[T],
    value: Option<T>,
    forward: bool,
) -> Option<T> {
    let mut order = vec![None];
    order.extend(values.iter().copied().map(Some));
    next_choice(&order, value, forward)
}

/// Returns the value after a choice among no value, `true`, and `false`.
fn next_bool(value: Option<bool>, forward: bool) -> Option<bool> {
    next_choice(&[None, Some(true), Some(false)], value, forward)
}

fn parse(input: &LineInput, name: &str) -> Result<Option<usize>, String> {
    let value = input.text().trim();
    if value.is_empty() {
        return Ok(None);
    }
    match value.parse::<usize>() {
        Ok(number) if number > 0 => Ok(Some(number)),
        _ => Err(text::settings_number(name)),
    }
}

/// Reads a size in MiB. A value that the user did not change keeps its exact bytes.
fn parse_bytes(
    input: &LineInput,
    name: &str,
    current: Option<usize>,
) -> Result<Option<usize>, String> {
    let value = parse(input, name)?;
    if value == mebibytes(current) {
        return Ok(current);
    }
    value.map_or(Ok(None), |value| {
        let bytes = value.checked_mul(MEBIBYTE);
        bytes.map(Some).ok_or_else(|| text::settings_number(name))
    })
}

/// Reads a time in seconds. Here the value 0 has a meaning: it stops the bell.
fn parse_seconds(input: &LineInput, name: &str) -> Result<Option<u64>, String> {
    let value = input.text().trim();
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse()
        .map(Some)
        .map_err(|_| text::settings_seconds(name))
}

impl SettingsForm {
    /// Opens on the project when one exists, as the connection form does. The focus
    /// starts on the sidebar.
    pub fn new(global: Preferences, project: Option<Preferences>) -> Self {
        let scope = if project.is_some() {
            Scope::Project
        } else {
            Scope::Global
        };
        Self {
            focus: Field::Sections,
            section: 0,
            scope,
            note: None,
            ai_busy: false,
            shortcuts: Shortcuts::new(
                project
                    .as_ref()
                    .and_then(|p| p.keyboard_mode)
                    .or(global.keyboard_mode),
            ),
            global_draft: Draft::of(&global, &global),
            project_draft: project.as_ref().map(|p| Draft::of(p, &global)),
            global,
            project,
        }
    }

    fn draft(&self) -> &Draft {
        match (self.scope, &self.project_draft) {
            (Scope::Project, Some(draft)) => draft,
            _ => &self.global_draft,
        }
    }

    fn draft_mut(&mut self) -> &mut Draft {
        match (self.scope, &mut self.project_draft) {
            (Scope::Project, Some(draft)) => draft,
            _ => &mut self.global_draft,
        }
    }

    pub fn has_project(&self) -> bool {
        self.project.is_some()
    }

    pub fn keyboard(&self) -> Option<KeyboardName> {
        self.draft().keyboard
    }

    pub fn reopen_tabs(&self) -> Option<bool> {
        self.draft().reopen_tabs
    }

    pub fn query_history(&self) -> Option<bool> {
        self.draft().query_history
    }

    pub fn format_json(&self) -> Option<bool> {
        self.draft().format_json
    }

    pub fn show_system_commands(&self) -> Option<bool> {
        self.draft().show_system_commands
    }

    pub fn cap_in_sql(&self) -> Option<bool> {
        self.draft().cap_in_sql
    }

    pub fn suggestion_keys(&self) -> Option<SuggestionKeys> {
        self.draft().suggestion_keys
    }

    pub fn theme(&self) -> Option<ThemeName> {
        self.draft().theme
    }

    pub fn mcp(&self) -> Option<bool> {
        self.draft().mcp
    }

    pub fn mcp_default(&self) -> Option<AgentDefault> {
        self.draft().mcp_default
    }

    pub fn mcp_max(&self) -> Option<AgentAccess> {
        self.draft().mcp_max
    }

    pub fn mcp_remember(&self) -> Option<bool> {
        self.draft().mcp_remember
    }

    /// Returns the theme that a save of this scope gives. A project value has priority
    /// over a global value.
    pub fn saved_theme(&self) -> Option<ThemeName> {
        let draft = self.draft().theme;
        match (self.scope, &self.project) {
            (Scope::Project, _) => draft.or(self.global.theme),
            (Scope::Global, Some(project)) => project.theme.or(draft),
            (Scope::Global, None) => draft,
        }
    }

    /// Returns the field of a number, with the cursor position.
    pub fn input(&self, field: Field) -> Option<&LineInput> {
        let draft = self.draft();
        match field {
            Field::PageSize => Some(&draft.page_size),
            Field::RowCap => Some(&draft.row_cap),
            Field::Budget => Some(&draft.budget),
            Field::UndoLimit => Some(&draft.undo_limit),
            Field::BellAfter => Some(&draft.bell_after),
            Field::AiModel => Some(&draft.ai.model),
            Field::AiEndpoint => Some(&draft.ai.endpoint),
            Field::AiPath => Some(&draft.ai.path),
            Field::AiKey => Some(&draft.ai.key),
            Field::AiContext => Some(&draft.ai.context),
            Field::AiOutput => Some(&draft.ai.output),
            Field::AiTimeout => Some(&draft.ai.timeout),
            Field::AiSimultaneous => Some(&draft.ai.simultaneous),
            _ => None,
        }
    }

    fn input_mut(&mut self) -> Option<&mut LineInput> {
        let field = self.focus;
        let draft = self.draft_mut();
        match field {
            Field::PageSize => Some(&mut draft.page_size),
            Field::RowCap => Some(&mut draft.row_cap),
            Field::Budget => Some(&mut draft.budget),
            Field::UndoLimit => Some(&mut draft.undo_limit),
            Field::BellAfter => Some(&mut draft.bell_after),
            Field::AiModel => Some(&mut draft.ai.model),
            Field::AiEndpoint => Some(&mut draft.ai.endpoint),
            Field::AiPath => Some(&mut draft.ai.path),
            Field::AiKey => Some(&mut draft.ai.key),
            Field::AiContext => Some(&mut draft.ai.context),
            Field::AiOutput => Some(&mut draft.ai.output),
            Field::AiTimeout => Some(&mut draft.ai.timeout),
            Field::AiSimultaneous => Some(&mut draft.ai.simultaneous),
            _ => None,
        }
    }

    /// Returns the text for a value that this scope does not declare.
    pub fn inherited(&self, field: Field) -> String {
        let from_global = self.scope == Scope::Project;
        let global = &self.global;
        let value = match field {
            Field::Keyboard => global.keyboard_mode.map(text::keyboard_name),
            Field::PageSize => global.page_size.map(|value| value.to_string()),
            Field::RowCap => global.row_cap.map(|value| value.to_string()),
            Field::CapInSql => global
                .cap_in_sql
                .map(|value| text::on_off(value).to_owned()),
            Field::Budget => mebibytes(global.grid_budget_bytes).map(|value| value.to_string()),
            Field::UndoLimit => mebibytes(global.undo_limit_bytes).map(|value| value.to_string()),
            Field::BellAfter => global.bell_after_seconds.map(|value| value.to_string()),
            Field::ReopenTabs => global
                .reopen_tabs
                .map(|value| text::on_off(value).to_owned()),
            Field::QueryHistory => global
                .query_history
                .map(|value| text::on_off(value).to_owned()),
            Field::ShowSystemCommands => global
                .show_system_commands
                .map(|value| text::on_off(value).to_owned()),
            Field::FormatJson => global
                .format_json
                .map(|value| text::on_off(value).to_owned()),
            Field::SuggestionKeys => global
                .suggestion_keys
                .map(|keys| text::suggestion_keys_name(keys).to_owned()),
            Field::Theme => global.theme.map(|name| text::theme_name(name).to_owned()),
            Field::Mcp => global.mcp.map(|value| text::on_off(value).to_owned()),
            Field::McpDefault => global
                .mcp_default_access
                .map(|value| text::agent_default_name(value).to_owned()),
            Field::McpMax => global
                .mcp_max_access
                .map(|value| text::agent_access_name(value).to_owned()),
            Field::McpRemember => global
                .mcp_remember_clients
                .map(|value| text::on_off(value).to_owned()),
            Field::AiEnabled => global
                .ai_completion
                .as_ref()
                .and_then(|p| p.enabled)
                .map(|value| text::on_off(value).to_owned()),
            _ => None,
        };
        match (from_global, value) {
            (true, Some(value)) => text::from_global(&value),
            _ if field == Field::ReopenTabs => {
                text::default_value(text::on_off(DEFAULT_REOPEN_TABS))
            }
            _ if field == Field::QueryHistory => {
                text::default_value(text::on_off(DEFAULT_QUERY_HISTORY))
            }
            _ if field == Field::ShowSystemCommands => {
                text::default_value(text::on_off(DEFAULT_SHOW_SYSTEM_COMMANDS))
            }
            _ if field == Field::FormatJson => {
                text::default_value(text::on_off(DEFAULT_FORMAT_JSON))
            }
            _ if field == Field::CapInSql => text::default_value(text::on_off(DEFAULT_CAP_IN_SQL)),
            _ if field == Field::BellAfter => {
                text::default_value(&DEFAULT_BELL_AFTER_SECONDS.to_string())
            }
            _ if field == Field::SuggestionKeys => {
                text::default_value(text::suggestion_keys_name(SuggestionKeys::default()))
            }
            _ if field == Field::UndoLimit => {
                text::default_value(&(DEFAULT_UNDO_LIMIT / MEBIBYTE).to_string())
            }
            _ if field == Field::Theme => {
                text::default_value(text::theme_name(ThemeName::default()))
            }
            _ if field == Field::Mcp => text::default_value(text::on_off(DEFAULT_MCP)),
            _ if field == Field::McpDefault => {
                text::default_value(text::agent_default_name(AgentDefault::default()))
            }
            _ if field == Field::McpMax => {
                text::default_value(text::agent_access_name(AgentAccess::default()))
            }
            _ if field == Field::McpRemember => {
                text::default_value(text::on_off(DEFAULT_MCP_REMEMBER_CLIENTS))
            }
            _ if field == Field::AiEnabled => text::default_value(text::on_off(false)),
            _ => text::VALUE_DEFAULT.to_owned(),
        }
    }

    /// Returns the places of the focus, in the order of `Tab`. The fields of the closed
    /// sections are not in the list.
    fn stops(&self) -> Vec<Field> {
        let mut stops = vec![Field::Sections];
        if self.has_project() {
            stops.push(Field::Scope);
        }
        let fields = SECTIONS[self.section].fields;
        stops.extend_from_slice(if fields.contains(&Field::ShortcutCommands) {
            self.shortcut_stops()
        } else {
            fields
        });
        stops.extend([Field::Save, Field::Cancel]);
        stops
    }

    fn move_focus(&mut self, forward: bool) {
        self.focus = next_choice(&self.stops(), self.focus, forward);
    }

    /// `Up` and `Down` open another section from the sidebar, and move the focus elsewhere.
    fn step(&mut self, forward: bool) {
        if self.focus == Field::Sections {
            let order: Vec<usize> = (0..SECTIONS.len()).collect();
            self.section = next_choice(&order, self.section, forward);
            self.shortcuts.editing = false;
        } else {
            self.move_focus(forward);
        }
    }

    /// The scope and the choices change with the side keys.
    fn change(&mut self, forward: bool) {
        match self.focus {
            Field::Scope if self.has_project() => {
                self.scope = match self.scope {
                    Scope::Global => Scope::Project,
                    Scope::Project => Scope::Global,
                };
            }
            Field::Keyboard => {
                let order = [
                    None,
                    Some(KeyboardName::Conventional),
                    Some(KeyboardName::Vim),
                ];
                self.draft_mut().keyboard = next_choice(&order, self.draft().keyboard, forward);
            }
            Field::ReopenTabs => {
                self.draft_mut().reopen_tabs = next_bool(self.draft().reopen_tabs, forward);
            }
            Field::QueryHistory => {
                self.draft_mut().query_history = next_bool(self.draft().query_history, forward);
            }
            Field::ShowSystemCommands => {
                self.draft_mut().show_system_commands =
                    next_bool(self.draft().show_system_commands, forward);
            }
            Field::FormatJson => {
                self.draft_mut().format_json = next_bool(self.draft().format_json, forward);
            }
            Field::CapInSql => {
                self.draft_mut().cap_in_sql = next_bool(self.draft().cap_in_sql, forward);
            }
            Field::SuggestionKeys => {
                let order = [
                    None,
                    Some(SuggestionKeys::Tab),
                    Some(SuggestionKeys::Enter),
                    Some(SuggestionKeys::Both),
                ];
                self.draft_mut().suggestion_keys =
                    next_choice(&order, self.draft().suggestion_keys, forward);
            }
            Field::Theme => {
                self.draft_mut().theme =
                    next_optional(&ThemeName::ALL, self.draft().theme, forward);
            }
            Field::Mcp => {
                self.draft_mut().mcp = next_bool(self.draft().mcp, forward);
            }
            Field::McpDefault => {
                self.draft_mut().mcp_default =
                    next_optional(&AgentDefault::ALL, self.draft().mcp_default, forward);
            }
            Field::McpMax => {
                self.draft_mut().mcp_max =
                    next_optional(&AgentAccess::ALL, self.draft().mcp_max, forward);
            }
            Field::McpRemember => {
                self.draft_mut().mcp_remember = next_bool(self.draft().mcp_remember, forward);
            }
            Field::AiEnabled => {
                self.draft_mut().ai.enabled = next_bool(self.draft().ai.enabled, forward);
                self.draft_mut().ai.touched = true;
            }
            Field::AiProfile => {
                let inherit = self.draft().ai.inherit;
                self.draft_mut().ai.inherit = !inherit;
                self.draft_mut().ai.touched = true;
            }
            Field::AiSource => {
                let source = self.draft().ai.source;
                self.draft_mut().ai.source = next_choice(&[0, 1, 2], source, forward);
                self.draft_mut().ai.inherit = false;
                self.draft_mut().ai.touched = true;
            }
            Field::AiReasoning => {
                let reasoning = self.draft().ai.reasoning;
                self.draft_mut().ai.reasoning = next_choice(&[0, 1, 2, 3, 4], reasoning, forward);
                self.draft_mut().ai.inherit = false;
                self.draft_mut().ai.touched = true;
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, value: &str) {
        if self.ai_busy {
            return;
        }
        if ai_text(self.focus) {
            self.draft_mut().ai.inherit = false;
            self.draft_mut().ai.touched = true;
            if let Some(input) = self.input_mut() {
                input.insert(value);
            }
            return;
        }
        let digits: String = value.chars().filter(char::is_ascii_digit).collect();
        if let Some(input) = self.input_mut() {
            input.insert(&digits);
        }
    }

    /// Puts the focus on one field. The mouse uses this.
    pub fn focus_on(&mut self, field: Field) {
        if !self.shortcuts.is_recording()
            || matches!(field, Field::ShortcutOk | Field::ShortcutCancel)
        {
            self.focus = field;
        }
    }

    /// Opens one section and puts the focus on the sidebar. The mouse uses this.
    pub fn open_section(&mut self, section: usize) {
        if self.shortcuts.is_recording() {
            return;
        }
        self.section = section;
        self.shortcuts.editing = false;
        self.focus = Field::Sections;
    }

    /// Acts on a click: a choice changes, a button runs.
    pub fn press(&mut self) -> SettingsRequest {
        if self.shortcut_press() {
            return SettingsRequest::None;
        }
        if self.ai_busy {
            return SettingsRequest::None;
        }
        match self.focus {
            Field::AiTest => SettingsRequest::AiTest,
            Field::Save => SettingsRequest::Save,
            Field::Cancel => SettingsRequest::Cancel,
            field if self.input(field).is_none() => {
                self.change(true);
                SettingsRequest::None
            }
            _ => SettingsRequest::None,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, vim: bool) -> SettingsRequest {
        if self.shortcuts.is_recording() {
            self.record_shortcut(key);
            return SettingsRequest::None;
        }
        self.note = None;
        if self.shortcut_key(key, vim) {
            return SettingsRequest::None;
        }
        // While a vault write or a test runs, only the close works.
        if self.ai_busy && key.code != KeyCode::Esc {
            return SettingsRequest::None;
        }
        match key.code {
            KeyCode::Esc => return SettingsRequest::Cancel,
            KeyCode::Tab => self.move_focus(true),
            KeyCode::BackTab => self.move_focus(false),
            KeyCode::Down => self.step(true),
            KeyCode::Up => self.step(false),
            KeyCode::Char('j') if vim && !ai_text(self.focus) => self.step(true),
            KeyCode::Char('k') if vim && !ai_text(self.focus) => self.step(false),
            KeyCode::Enter if self.focus == Field::AiTest => return SettingsRequest::AiTest,
            KeyCode::Enter if self.focus == Field::Save => return SettingsRequest::Save,
            KeyCode::Enter if self.focus == Field::Cancel => return SettingsRequest::Cancel,
            KeyCode::Enter => self.move_focus(true),
            KeyCode::Left if self.input(self.focus).is_none() => self.change(false),
            KeyCode::Right if self.input(self.focus).is_none() => self.change(true),
            KeyCode::Char('h') if vim && self.input(self.focus).is_none() => self.change(false),
            KeyCode::Char('l') if vim && self.input(self.focus).is_none() => self.change(true),
            KeyCode::Char(c) if !c.is_ascii_digit() && !ai_text(self.focus) => {}
            _ => {
                // An edit of a provider field makes the block custom.
                if ai_owned(self.focus) {
                    self.draft_mut().ai.touched = true;
                }
                if ai_text(self.focus) {
                    self.draft_mut().ai.inherit = false;
                }
                if let Some(input) = self.input_mut() {
                    input.on_key(key);
                }
            }
        }
        SettingsRequest::None
    }

    pub fn ai_enabled(&self) -> Option<bool> {
        self.draft().ai.enabled
    }

    pub fn ai_inherit(&self) -> bool {
        self.draft().ai.inherit
    }

    pub fn ai_source(&self) -> usize {
        self.draft().ai.source
    }

    /// Returns the index of the reasoning effort in `Reasoning::ALL`.
    pub fn ai_reasoning(&self) -> usize {
        self.draft().ai.reasoning
    }

    /// Returns the masked key text, or the place of the saved key.
    pub fn ai_key(&self) -> String {
        let ai = &self.draft().ai;
        if ai.key.text().is_empty() {
            if matches!(ai.provider.credential, Credential::Vault { .. }) {
                "(saved key)".into()
            } else {
                String::new()
            }
        } else {
            "*".repeat(ai.key.text().chars().count().min(24))
        }
    }

    /// Returns the typed key before it reaches the vault.
    pub fn ai_key_text(&self) -> &str {
        self.draft().ai.key.text()
    }

    /// Returns the typed key when a save must write it to the system vault.
    pub fn ai_new_key(&self) -> Option<String> {
        let ai = &self.draft().ai;
        if ai.source == 1 && !ai.inherit && !ai.key.text().is_empty() {
            Some(ai.key.text().to_owned())
        } else {
            None
        }
    }

    /// Builds the autocomplete preferences of the draft. Editing contacts no provider.
    pub fn ai_preferences(&self) -> Result<crate::ai::Preferences, String> {
        let ai = &self.draft().ai;
        if ai.key.text().len() > 65536 {
            return Err("The API key exceeds 64 KiB. Supply only the key.".into());
        }
        let simultaneous: u8 = ai
            .simultaneous
            .text()
            .parse()
            .map_err(|_| "Set a count from 1 to 9.")?;
        if !(1..=9).contains(&simultaneous) {
            return Err("Set a count from 1 to 9.".into());
        }
        if ai.inherit {
            return Ok(crate::ai::Preferences {
                enabled: ai.enabled,
                max_simultaneous: Some(simultaneous),
                provider: None,
            });
        }
        let mut provider = ai.provider.clone();
        provider.model = ai.model.text().to_owned();
        provider.base_url = ai.endpoint.text().to_owned();
        provider.context_window_tokens = ai
            .context
            .text()
            .parse()
            .map_err(|_| "Set a valid context size.")?;
        provider.max_output_tokens = ai
            .output
            .text()
            .parse()
            .map_err(|_| "Set a valid output size.")?;
        provider.timeout_seconds = ai
            .timeout
            .text()
            .parse()
            .map_err(|_| "Set a valid timeout.")?;
        provider.reasoning_effort = Reasoning::ALL
            .get(ai.reasoning)
            .copied()
            .unwrap_or(Reasoning::Off);
        provider.credential = match ai.source {
            0 => Credential::None,
            2 => Credential::File {
                path: ai.path.text().to_owned(),
            },
            _ => {
                if !ai.key.text().is_empty() {
                    Credential::Vault {
                        id: uuid::Uuid::new_v4().to_string(),
                        endpoint: provider.endpoint()?,
                    }
                } else {
                    match &provider.credential {
                        Credential::Vault { .. } => provider.credential.clone(),
                        _ => return Err("Insert a key for the system vault.".into()),
                    }
                }
            }
        };
        provider.validate()?;
        Ok(crate::ai::Preferences {
            enabled: ai.enabled,
            max_simultaneous: Some(simultaneous),
            provider: Some(provider),
        })
    }

    /// Returns the provider that a connection test uses.
    pub fn ai_test_provider(&self) -> Result<Provider, String> {
        self.ai_preferences()?
            .provider
            .or_else(|| Some(self.draft().ai.provider.clone()))
            .ok_or_else(|| "Configure a provider before testing.".into())
    }

    /// Replaces the autocomplete fields with one saved block.
    pub fn set_ai_preferences(&mut self, preferences: crate::ai::Preferences) {
        let global = self.global.ai_completion.clone();
        let effective = crate::ai::Preferences::resolve(global.as_ref(), Some(&preferences));
        self.draft_mut().ai = AiDraft::of(Some(&preferences), &effective);
        self.draft_mut().ai.touched = true;
    }

    pub fn preferences(&self) -> Result<Preferences, String> {
        let draft = self.draft();
        let base = match (self.scope, &self.project) {
            (Scope::Project, Some(project)) => project,
            _ => &self.global,
        };
        let mut preferences = base.clone();
        preferences.keyboard_mode = draft.keyboard;
        preferences.reopen_tabs = draft.reopen_tabs;
        preferences.query_history = draft.query_history;
        preferences.format_json = draft.format_json;
        preferences.show_system_commands = draft.show_system_commands;
        preferences.cap_in_sql = draft.cap_in_sql;
        preferences.suggestion_keys = draft.suggestion_keys;
        preferences.theme = draft.theme;
        preferences.mcp = draft.mcp;
        preferences.mcp_default_access = draft.mcp_default;
        preferences.mcp_max_access = draft.mcp_max;
        preferences.mcp_remember_clients = draft.mcp_remember;
        preferences.ai_completion = if draft.ai.touched {
            Some(self.ai_preferences()?)
        } else {
            draft.ai.saved.clone()
        };
        preferences.keymap = draft.keymap.clone();
        self.validate_shortcuts(&preferences)?;
        preferences.page_size = parse(&draft.page_size, text::FIELD_PAGE_SIZE)?;
        preferences.row_cap = parse(&draft.row_cap, text::FIELD_ROW_CAP)?;
        preferences.bell_after_seconds = parse_seconds(&draft.bell_after, text::FIELD_BELL_AFTER)?;
        preferences.grid_budget_bytes =
            parse_bytes(&draft.budget, text::FIELD_BUDGET, base.grid_budget_bytes)?;
        preferences.undo_limit_bytes = parse_bytes(
            &draft.undo_limit,
            text::FIELD_UNDO_LIMIT,
            base.undo_limit_bytes,
        )?;
        Ok(preferences)
    }
}
