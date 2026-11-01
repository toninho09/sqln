//! The settings dialog. It edits the preferences of one scope and writes nothing
//! before the save.

use crossterm::event::{KeyCode, KeyEvent};

use crate::input::SuggestionKeys;
use crate::line_input::LineInput;
use crate::store::Scope;
use crate::store::config::{DEFAULT_REOPEN_TABS, KeyboardName, Preferences, ThemeName};
use crate::ui::text;

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
    Budget,
    ReopenTabs,
    SuggestionKeys,
    Theme,
    Save,
    Cancel,
}

/// One group of the sidebar.
pub struct Section {
    pub title: &'static str,
    pub fields: &'static [Field],
}

/// The sidebar. To add a group, add a row here.
pub const SECTIONS: [Section; 4] = [
    Section {
        title: text::SECTION_GENERAL,
        fields: &[Field::Keyboard, Field::ReopenTabs],
    },
    Section {
        title: text::SECTION_EDITOR,
        fields: &[Field::SuggestionKeys],
    },
    Section {
        title: text::SECTION_RESULTS,
        fields: &[Field::PageSize, Field::RowCap, Field::Budget],
    },
    Section {
        title: text::SECTION_APPEARANCE,
        fields: &[Field::Theme],
    },
];

pub enum SettingsRequest {
    None,
    Cancel,
    Save,
}

/// The fields of one scope. An empty field removes the definition from the scope.
#[derive(Clone)]
struct Draft {
    keyboard: Option<KeyboardName>,
    page_size: LineInput,
    row_cap: LineInput,
    budget: LineInput,
    reopen_tabs: Option<bool>,
    suggestion_keys: Option<SuggestionKeys>,
    theme: Option<ThemeName>,
}

fn number_text(value: Option<usize>) -> LineInput {
    let mut input = LineInput::default();
    input.set(&value.map_or_else(String::new, |value| value.to_string()));
    input
}

impl Draft {
    fn of(preferences: &Preferences) -> Self {
        let budget = preferences
            .grid_budget_bytes
            .map(|bytes| bytes.div_ceil(MEBIBYTE));
        Self {
            keyboard: preferences.keyboard_mode,
            page_size: number_text(preferences.page_size),
            row_cap: number_text(preferences.row_cap),
            budget: number_text(budget),
            reopen_tabs: preferences.reopen_tabs,
            suggestion_keys: preferences.suggestion_keys,
            theme: preferences.theme,
        }
    }
}

pub struct SettingsForm {
    pub focus: Field,
    /// The index of the open section in `SECTIONS`.
    pub section: usize,
    pub scope: Scope,
    pub note: Option<String>,
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
            global_draft: Draft::of(&global),
            project_draft: project.as_ref().map(Draft::of),
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

    pub fn suggestion_keys(&self) -> Option<SuggestionKeys> {
        self.draft().suggestion_keys
    }

    pub fn theme(&self) -> Option<ThemeName> {
        self.draft().theme
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
            Field::Budget => global
                .grid_budget_bytes
                .map(|bytes| bytes.div_ceil(MEBIBYTE).to_string()),
            Field::ReopenTabs => global
                .reopen_tabs
                .map(|value| text::on_off(value).to_owned()),
            Field::SuggestionKeys => global
                .suggestion_keys
                .map(|keys| text::suggestion_keys_name(keys).to_owned()),
            Field::Theme => global.theme.map(|name| text::theme_name(name).to_owned()),
            _ => None,
        };
        match (from_global, value) {
            (true, Some(value)) => text::from_global(&value),
            _ if field == Field::ReopenTabs => {
                text::default_value(text::on_off(DEFAULT_REOPEN_TABS))
            }
            _ if field == Field::SuggestionKeys => {
                text::default_value(text::suggestion_keys_name(SuggestionKeys::default()))
            }
            _ if field == Field::Theme => {
                text::default_value(text::theme_name(ThemeName::default()))
            }
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
        stops.extend_from_slice(SECTIONS[self.section].fields);
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
                let order = [None, Some(true), Some(false)];
                self.draft_mut().reopen_tabs =
                    next_choice(&order, self.draft().reopen_tabs, forward);
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
            _ => {}
        }
    }

    pub fn paste(&mut self, value: &str) {
        let digits: String = value.chars().filter(char::is_ascii_digit).collect();
        if let Some(input) = self.input_mut() {
            input.insert(&digits);
        }
    }

    /// Puts the focus on one field. The mouse uses this.
    pub fn focus_on(&mut self, field: Field) {
        self.focus = field;
    }

    /// Opens one section and puts the focus on the sidebar. The mouse uses this.
    pub fn open_section(&mut self, section: usize) {
        self.section = section;
        self.focus = Field::Sections;
    }

    /// Acts on a click: a choice changes, a button runs.
    pub fn press(&mut self) -> SettingsRequest {
        match self.focus {
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
        self.note = None;
        match key.code {
            KeyCode::Esc => return SettingsRequest::Cancel,
            KeyCode::Tab => self.move_focus(true),
            KeyCode::BackTab => self.move_focus(false),
            KeyCode::Down => self.step(true),
            KeyCode::Up => self.step(false),
            KeyCode::Char('j') if vim => self.step(true),
            KeyCode::Char('k') if vim => self.step(false),
            KeyCode::Enter if self.focus == Field::Save => return SettingsRequest::Save,
            KeyCode::Enter if self.focus == Field::Cancel => return SettingsRequest::Cancel,
            KeyCode::Enter => self.move_focus(true),
            KeyCode::Left if self.input(self.focus).is_none() => self.change(false),
            KeyCode::Right if self.input(self.focus).is_none() => self.change(true),
            KeyCode::Char('h') if vim && self.input(self.focus).is_none() => self.change(false),
            KeyCode::Char('l') if vim && self.input(self.focus).is_none() => self.change(true),
            KeyCode::Char(c) if !c.is_ascii_digit() => {}
            _ => {
                if let Some(input) = self.input_mut() {
                    input.on_key(key);
                }
            }
        }
        SettingsRequest::None
    }

    /// Returns the preferences of the scope. The other values of the file, such as the
    /// `keymap`, stay, and a budget that the user did not change keeps its exact bytes.
    pub fn preferences(&self) -> Result<Preferences, String> {
        let draft = self.draft();
        let base = match (self.scope, &self.project) {
            (Scope::Project, Some(project)) => project,
            _ => &self.global,
        };
        let mut preferences = base.clone();
        preferences.keyboard_mode = draft.keyboard;
        preferences.reopen_tabs = draft.reopen_tabs;
        preferences.suggestion_keys = draft.suggestion_keys;
        preferences.theme = draft.theme;
        preferences.page_size = parse(&draft.page_size, text::FIELD_PAGE_SIZE)?;
        preferences.row_cap = parse(&draft.row_cap, text::FIELD_ROW_CAP)?;
        let budget = parse(&draft.budget, text::FIELD_BUDGET)?;
        let same = budget == base.grid_budget_bytes.map(|bytes| bytes.div_ceil(MEBIBYTE));
        if !same {
            let bytes = budget.map(|value| value.checked_mul(MEBIBYTE));
            preferences.grid_budget_bytes = match bytes {
                Some(None) => return Err(text::settings_number(text::FIELD_BUDGET)),
                Some(value) => value,
                None => None,
            };
        }
        Ok(preferences)
    }
}
