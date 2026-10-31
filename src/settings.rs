//! The settings dialog. It edits the preferences of one scope and writes nothing
//! before the save.

use crossterm::event::{KeyCode, KeyEvent};

use crate::line_input::LineInput;
use crate::store::Scope;
use crate::store::config::{KeyboardName, Preferences};
use crate::ui::text;

const MEBIBYTE: usize = 1024 * 1024;
/// The defaults of `Settings`, for the note of a field without a value.
pub const DEFAULT_KEYBOARD: KeyboardName = KeyboardName::Conventional;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Scope,
    Keyboard,
    PageSize,
    RowCap,
    Budget,
    Save,
    Cancel,
}

const FIELDS: [Field; 7] = [
    Field::Scope,
    Field::Keyboard,
    Field::PageSize,
    Field::RowCap,
    Field::Budget,
    Field::Save,
    Field::Cancel,
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
}

fn number_text(value: Option<usize>) -> LineInput {
    let mut input = LineInput::default();
    input.set(&value.map_or_else(String::new, |value| value.to_string()));
    input
}

impl Draft {
    fn of(preferences: &Preferences) -> Self {
        let budget = preferences.grid_budget_bytes.map(|bytes| bytes.div_ceil(MEBIBYTE));
        Self {
            keyboard: preferences.keyboard_mode,
            page_size: number_text(preferences.page_size),
            row_cap: number_text(preferences.row_cap),
            budget: number_text(budget),
        }
    }
}

pub struct SettingsForm {
    pub focus: Field,
    pub scope: Scope,
    pub note: Option<String>,
    global: Preferences,
    project: Option<Preferences>,
    global_draft: Draft,
    project_draft: Option<Draft>,
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
    /// Opens on the project when one exists, as the connection form does.
    pub fn new(global: Preferences, project: Option<Preferences>) -> Self {
        let scope = if project.is_some() { Scope::Project } else { Scope::Global };
        let focus = if project.is_some() { Field::Scope } else { Field::Keyboard };
        Self {
            focus,
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
            Field::Budget => global.grid_budget_bytes.map(|bytes| bytes.div_ceil(MEBIBYTE).to_string()),
            _ => None,
        };
        match (from_global, value) {
            (true, Some(value)) => text::from_global(&value),
            _ => text::VALUE_DEFAULT.to_owned(),
        }
    }

    fn move_focus(&mut self, step: isize) {
        let count = isize::try_from(FIELDS.len()).unwrap_or(1);
        let mut index = FIELDS.iter().position(|field| *field == self.focus).unwrap_or(0);
        for _ in 0..FIELDS.len() {
            let current = isize::try_from(index).unwrap_or(0);
            index = usize::try_from((current + step).rem_euclid(count)).unwrap_or(0);
            if FIELDS[index] != Field::Scope || self.has_project() {
                break;
            }
        }
        self.focus = FIELDS[index];
    }

    /// The scope and the keyboard change with the side keys.
    fn change(&mut self, forward: bool) {
        match self.focus {
            Field::Scope if self.has_project() => {
                self.scope = match self.scope {
                    Scope::Global => Scope::Project,
                    Scope::Project => Scope::Global,
                };
            }
            Field::Keyboard => {
                let order = [None, Some(KeyboardName::Conventional), Some(KeyboardName::Vim)];
                let index = order.iter().position(|item| *item == self.draft().keyboard).unwrap_or(0);
                let next = if forward { index + 1 } else { index + order.len() - 1 };
                self.draft_mut().keyboard = order[next % order.len()];
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

    pub fn on_key(&mut self, key: KeyEvent, vim: bool) -> SettingsRequest {
        self.note = None;
        match key.code {
            KeyCode::Esc => return SettingsRequest::Cancel,
            KeyCode::Tab | KeyCode::Down => self.move_focus(1),
            KeyCode::BackTab | KeyCode::Up => self.move_focus(-1),
            KeyCode::Char('j') if vim => self.move_focus(1),
            KeyCode::Char('k') if vim => self.move_focus(-1),
            KeyCode::Enter if self.focus == Field::Save => return SettingsRequest::Save,
            KeyCode::Enter if self.focus == Field::Cancel => return SettingsRequest::Cancel,
            KeyCode::Enter => self.move_focus(1),
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
