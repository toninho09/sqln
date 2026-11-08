//! Custom theme management and the theme editor of the settings dialog.
//! Nothing reaches a file before the save of the dialog.

use std::collections::{BTreeMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent};

use super::{Field, SettingsForm, next_choice};
use crate::line_input::LineInput;
use crate::store::Scope;
use crate::store::config::ThemeName;
use crate::store::themes as files;
use crate::ui::schemes::{self, COLOR_ROLES, Scheme};
use crate::ui::text;
use crate::ui::theme::{CustomTheme, ThemeCatalog};
use ratatui::style::Color;

/// The fields of the Appearance section without the editor.
pub const FIELDS: &[Field] = &[
    Field::Theme,
    Field::Themes,
    Field::ThemeNew,
    Field::ThemeEdit,
    Field::ThemeDelete,
];
const EDIT_FIELDS: &[Field] = &[
    Field::ThemeName,
    Field::ThemeColors,
    Field::ThemeDone,
    Field::ThemeBack,
];
const EDIT_NEW_FIELDS: &[Field] = &[
    Field::ThemeName,
    Field::ThemeBase,
    Field::ThemeColors,
    Field::ThemeDone,
    Field::ThemeBack,
];

/// The files to write at a save of the dialog.
#[derive(Default)]
pub struct ThemeChanges {
    /// The themes to write, each to the folder of its scope.
    pub saved: Vec<CustomTheme>,
    /// The files to remove.
    pub removed: Vec<CustomTheme>,
}

impl ThemeChanges {
    pub fn is_empty(&self) -> bool {
        self.saved.is_empty() && self.removed.is_empty()
    }
}

/// The custom themes of the settings dialog.
pub struct Themes {
    /// The themes of both scopes at the open of the dialog.
    catalog: ThemeCatalog,
    /// The ids that a connection uses.
    used: HashSet<String>,
    /// The themes to write at a save, by id.
    changed: BTreeMap<String, CustomTheme>,
    /// The files to remove at a save.
    removed: Vec<CustomTheme>,
    /// The selected row of the list.
    selected: usize,
    /// The open editor.
    editor: Option<ThemeEditor>,
}

impl Themes {
    pub fn new(catalog: ThemeCatalog, used: HashSet<String>) -> Self {
        Self {
            catalog,
            used,
            changed: BTreeMap::new(),
            removed: Vec::new(),
            selected: 0,
            editor: None,
        }
    }

    /// The themes that the list and the choices show, after the drafts of the
    /// dialog: the loaded themes, the changes, without the removals.
    pub fn visible(&self) -> Vec<CustomTheme> {
        let removed: HashSet<&str> = self.removed.iter().map(|theme| theme.id.as_str()).collect();
        let mut all: BTreeMap<String, CustomTheme> = BTreeMap::new();
        for theme in self.catalog.list() {
            all.insert(theme.id.clone(), theme.clone());
        }
        for (id, theme) in &self.changed {
            all.insert(id.clone(), theme.clone());
        }
        all.into_values()
            .filter(|theme| !removed.contains(theme.id.as_str()))
            .collect()
    }

    /// The visible themes as a catalog, for names in lists.
    pub fn view(&self) -> ThemeCatalog {
        let mut view = ThemeCatalog::default();
        for theme in self.visible() {
            view.insert(theme);
        }
        view
    }

    /// The theme choices of the settings: the built-in themes, then the customs.
    pub fn choices(&self) -> Vec<ThemeName> {
        let mut choices: Vec<ThemeName> = ThemeName::ALL.to_vec();
        choices.extend(
            self.visible()
                .iter()
                .map(|theme| ThemeName::Custom(theme.id.clone())),
        );
        choices
    }

    /// The themes that a new theme can copy: the built-in palettes and the
    /// customs, without `Auto` and `Ansi`, which follow the terminal.
    pub fn bases(&self) -> Vec<ThemeName> {
        let mut bases: Vec<ThemeName> = ThemeName::ALL
            .iter()
            .filter(|name| !matches!(name, ThemeName::Auto | ThemeName::Ansi))
            .cloned()
            .collect();
        bases.extend(
            self.visible()
                .iter()
                .map(|theme| ThemeName::Custom(theme.id.clone())),
        );
        bases
    }

    /// The palette of a base choice.
    pub fn base_scheme(&self, base: &ThemeName) -> Option<Scheme> {
        match base {
            ThemeName::Custom(id) => self.scheme_of(id),
            name => Scheme::named(name),
        }
    }

    /// The palette that a name resolves to at this moment, for the start of a
    /// new theme. `Auto`, `Ansi`, and a missing theme give the first base.
    pub fn base_of(&self, name: Option<&ThemeName>) -> (usize, Scheme) {
        let bases = self.bases();
        let found = name
            .filter(|name| !matches!(name, ThemeName::Auto | ThemeName::Ansi))
            .and_then(|name| {
                let scheme = self.base_scheme(name)?;
                bases
                    .iter()
                    .position(|base| base == name)
                    .map(|index| (index, scheme))
            });
        found.unwrap_or_else(|| (0, self.base_scheme(&bases[0]).unwrap_or(schemes::ASTRODARK)))
    }

    /// The palette of an id as the dialog holds it now: the editor, then a
    /// change, then the loaded theme.
    pub fn scheme_of(&self, id: &str) -> Option<Scheme> {
        if let Some(editor) = &self.editor
            && editor.draft_id() == id
        {
            return Some(editor.scheme);
        }
        if let Some(theme) = self.changed.get(id) {
            return Some(theme.scheme);
        }
        self.catalog.scheme(id)
    }

    /// True for an id that a removal of the dialog takes away.
    pub fn gone(&self, id: &str) -> bool {
        self.removed.iter().any(|theme| theme.id == id)
    }

    pub fn is_editing(&self) -> bool {
        self.editor.is_some()
    }

    /// True while the editor builds a theme that no file holds.
    pub fn is_new(&self) -> bool {
        self.editor
            .as_ref()
            .is_some_and(|editor| editor.id.is_none())
    }

    /// The ids that a connection uses.
    pub fn in_use(&self, id: &str) -> bool {
        self.used.contains(id)
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn editor(&self) -> Option<&ThemeEditor> {
        self.editor.as_ref()
    }

    pub fn editor_mut(&mut self) -> Option<&mut ThemeEditor> {
        self.editor.as_mut()
    }

    /// Moves the selection of the list.
    pub fn move_selection(&mut self, delta: isize) {
        let len = self.visible().len();
        self.selected = if len == 0 {
            0
        } else {
            let next = isize::try_from(self.selected).unwrap_or(isize::MAX);
            let last = isize::try_from(len - 1).unwrap_or(isize::MAX);
            next.saturating_add(delta).clamp(0, last) as usize
        };
    }

    pub fn select(&mut self, id: &str) {
        self.selected = self
            .visible()
            .iter()
            .position(|theme| theme.id == id)
            .unwrap_or(self.selected);
    }

    /// Starts the editor on a copy of one base.
    pub fn start_new(&mut self, base: usize, scheme: Scheme) {
        self.editor = Some(ThemeEditor::new(None, base, scheme));
    }

    /// Starts the editor on the selected theme.
    pub fn start_edit(&mut self) -> Result<(), String> {
        let Some(theme) = self.visible().get(self.selected).cloned() else {
            return Err(text::THEME_NONE.to_owned());
        };
        let bases = self.bases();
        let base = bases
            .iter()
            .position(|base| matches!(base, ThemeName::Custom(id) if id == &theme.id))
            .unwrap_or(0);
        let mut editor = ThemeEditor::new(Some(theme.id.clone()), base, theme.scheme);
        editor.name.set(&theme.name);
        self.editor = Some(editor);
        Ok(())
    }

    /// Leaves the editor without the draft.
    pub fn cancel(&mut self) {
        self.editor = None;
    }

    /// Moves the focused color of the editor.
    pub fn move_cursor(&mut self, delta: isize) {
        if let Some(editor) = &mut self.editor {
            let last = isize::try_from(COLOR_ROLES.len() - 1).unwrap_or(isize::MAX);
            let next = isize::try_from(editor.cursor).unwrap_or(isize::MAX);
            editor.cursor = next.saturating_add(delta).clamp(0, last) as usize;
        }
    }

    /// Applies the text of the focused color to the palette, when it parses.
    pub fn after_edit(&mut self) {
        if let Some(editor) = &mut self.editor
            && let Some(color) = schemes::color_of_text(editor.hexes[editor.cursor].text())
        {
            editor.scheme.set_color(editor.cursor, color);
        }
    }

    /// Changes the base of a new theme: the new palette replaces the colors.
    pub fn change_base(&mut self, forward: bool) {
        let bases = self.bases();
        if bases.len() < 2 {
            return;
        }
        let Some(current) = self.editor.as_ref().map(|editor| {
            bases
                .get(editor.base)
                .cloned()
                .unwrap_or_else(|| bases[0].clone())
        }) else {
            return;
        };
        let base = next_choice(&bases, current, forward);
        let Some(scheme) = self.base_scheme(&base) else {
            return;
        };
        let index = bases.iter().position(|item| *item == base).unwrap_or(0);
        let hexes = ThemeEditor::hexes_of(&scheme);
        if let Some(editor) = &mut self.editor {
            editor.base = index;
            editor.scheme = scheme;
            editor.hexes = hexes;
        }
    }

    /// Takes the draft of the editor into the changes of the dialog. The result
    /// carries a warning when the palette has places of low contrast.
    pub fn finish(&mut self, scope: Scope) -> Result<Option<String>, String> {
        let (name, id_of_edit, hexes, mut scheme) = {
            let Some(editor) = self.editor.as_ref() else {
                return Ok(None);
            };
            (
                editor.name.text().trim().to_owned(),
                editor.id.clone(),
                editor
                    .hexes
                    .iter()
                    .map(|hex| hex.text().to_owned())
                    .collect::<Vec<_>>(),
                editor.scheme,
            )
        };
        if name.is_empty() {
            return Err("The theme needs a name.".to_owned());
        }
        let id = id_of_edit.clone().unwrap_or_else(|| files::slug(&name));
        if !files::valid_id(&id) {
            return Err(text::THEME_INVALID_ID.to_owned());
        }
        if self
            .visible()
            .iter()
            .any(|theme| theme.id == id && Some(theme.id.clone()) != id_of_edit)
        {
            return Err(text::THEME_ID_TAKEN.to_owned());
        }
        for (index, role) in COLOR_ROLES.iter().enumerate() {
            match schemes::color_of_text(&hexes[index]) {
                Some(color) => scheme.set_color(index, color),
                None => return Err(text::theme_hex_invalid(role.label)),
            }
        }
        let before = self.changed.get(&id).or_else(|| self.catalog.get(&id));
        let theme = CustomTheme {
            scope: before.map_or(scope, |before| before.scope),
            file: before.and_then(|before| before.file),
            id: id.clone(),
            name,
            scheme,
        };
        // A theme that the dialog removed and that comes back writes again.
        self.removed.retain(|removed| removed.id != id);
        self.changed.insert(id.clone(), theme);
        self.selected = self
            .visible()
            .iter()
            .position(|theme| theme.id == id)
            .unwrap_or(0);
        self.editor = None;
        let failures = schemes::contrast_failures(&scheme);
        Ok((!failures.is_empty()).then(|| text::theme_contrast(failures.len())))
    }

    /// Takes the selected theme away from the list. The caller checked that
    /// nothing uses the theme.
    pub fn delete(&mut self) -> Result<(), String> {
        let Some(theme) = self.visible().get(self.selected).cloned() else {
            return Err(text::THEME_NONE.to_owned());
        };
        let theme = theme.clone();
        self.changed.remove(&theme.id);
        if theme.file.is_some() {
            self.removed.push(theme);
        }
        self.selected = self.selected.min(self.visible().len().saturating_sub(1));
        Ok(())
    }

    /// The file changes of the dialog.
    pub fn changes(&self) -> ThemeChanges {
        ThemeChanges {
            saved: self.changed.values().cloned().collect(),
            removed: self
                .removed
                .iter()
                .filter(|theme| theme.file.is_some())
                .cloned()
                .collect(),
        }
    }

    /// Keeps the hash of a written file, so a later write of the same save
    /// finds the state that it wrote.
    pub fn written(&mut self, id: &str, hash: u64) {
        if let Some(theme) = self.changed.get_mut(id) {
            theme.file = Some(hash);
        }
    }
}

/// The editor of one theme. The palette changes as the hex texts parse, so the
/// preview of the screen follows each key.
pub struct ThemeEditor {
    /// The id of the theme under edit. A new theme takes its id from its name.
    id: Option<String>,
    pub name: LineInput,
    /// The index of the base in the choices of `Themes::bases`.
    pub base: usize,
    /// The hex text of each role of `COLOR_ROLES`.
    pub(super) hexes: Vec<LineInput>,
    /// The palette of the last valid text of each role.
    scheme: Scheme,
    /// The focused color role.
    pub cursor: usize,
}

impl ThemeEditor {
    fn new(id: Option<String>, base: usize, scheme: Scheme) -> Self {
        let hexes = Self::hexes_of(&scheme);
        Self {
            id,
            name: LineInput::default(),
            base,
            hexes,
            scheme,
            cursor: 0,
        }
    }

    fn hexes_of(scheme: &Scheme) -> Vec<LineInput> {
        COLOR_ROLES
            .iter()
            .map(|role| {
                let mut input = LineInput::default();
                input.set(&schemes::hex_text((role.get)(scheme)));
                input
            })
            .collect()
    }

    /// The id that the preview resolves while the editor is open.
    pub fn draft_id(&self) -> String {
        self.id.clone().unwrap_or_else(|| {
            let id = files::slug(self.name.text());
            if id.is_empty() {
                "draft".to_owned()
            } else {
                id
            }
        })
    }

    /// The hex text and the palette color of one color role.
    pub fn hex(&self, role: usize) -> (&LineInput, Color) {
        (&self.hexes[role], self.scheme.color(role))
    }
}

impl SettingsForm {
    /// The places of the focus of the Appearance section.
    pub(crate) fn theme_stops(&self) -> &'static [Field] {
        if self.themes.is_editing() {
            if self.themes.is_new() {
                EDIT_NEW_FIELDS
            } else {
                EDIT_FIELDS
            }
        } else {
            FIELDS
        }
    }

    /// The theme that the screen shows while the dialog is open: the editor
    /// draft, then the choice of the scope.
    pub fn preview_theme(&self) -> Option<ThemeName> {
        if let Some(editor) = self.themes.editor() {
            return Some(ThemeName::Custom(editor.draft_id()));
        }
        self.saved_theme()
    }

    /// The palette of an id as the dialog holds it.
    pub fn theme_scheme(&self, id: &str) -> Option<Scheme> {
        self.themes.scheme_of(id)
    }

    /// True for an id that a removal of the dialog takes away.
    pub fn theme_gone(&self, id: &str) -> bool {
        self.themes.gone(id)
    }

    /// The file changes of the dialog.
    pub fn theme_changes(&self) -> ThemeChanges {
        self.themes.changes()
    }

    /// Keeps the hash of a written theme file.
    pub fn theme_written(&mut self, id: &str, hash: u64) {
        self.themes.written(id, hash);
    }

    /// The label of a theme name with the themes of the dialog.
    pub fn theme_label(&self, name: &ThemeName) -> String {
        text::theme_label(name, &self.themes.view())
    }

    /// The themes that the list shows.
    pub fn theme_list(&self) -> Vec<CustomTheme> {
        self.themes.visible()
    }

    pub fn theme_selected(&self) -> usize {
        self.themes.selected()
    }

    pub fn theme_is_editing(&self) -> bool {
        self.themes.is_editing()
    }

    pub fn theme_is_new(&self) -> bool {
        self.themes.is_new()
    }

    /// The id that the editor draft resolves to.
    pub fn theme_editor_id(&self) -> String {
        self.themes
            .editor()
            .map_or_else(String::new, |editor| editor.draft_id())
    }

    /// The name of the editor draft, or the place of a theme under edit.
    pub fn theme_editor_name(&self) -> &LineInput {
        let editor = self.themes.editor().expect("the theme editor is open");
        &editor.name
    }

    /// The label of the base choice of the editor.
    pub fn theme_base_label(&self) -> String {
        let editor = self.themes.editor().expect("the theme editor is open");
        let bases = self.themes.bases();
        bases
            .get(editor.base)
            .map(|base| text::theme_label(base, &self.themes.view()))
            .unwrap_or_default()
    }

    /// The focused color role of the editor.
    pub fn theme_cursor(&self) -> usize {
        self.themes.editor().map_or(0, |editor| editor.cursor)
    }

    /// The hex input and the color of one role of the editor.
    pub fn theme_hex(&self, role: usize) -> Option<(&LineInput, Color)> {
        let editor = self.themes.editor()?;
        Some(editor.hex(role))
    }

    /// The count of color roles of the palette.
    pub fn theme_role_count(&self) -> usize {
        COLOR_ROLES.len()
    }

    /// The label of one color role.
    pub fn theme_role_label(role: usize) -> &'static str {
        COLOR_ROLES[role].label
    }

    /// The warning of the palette under edit, when it has low contrast.
    pub fn theme_contrast_warning(&self) -> Option<String> {
        let editor = self.themes.editor()?;
        let failures = schemes::contrast_failures(&editor.scheme);
        (!failures.is_empty()).then(|| text::theme_contrast(failures.len()))
    }

    /// The label of the value of a field of the themes.
    pub fn theme_value(&self, field: Field) -> String {
        match field {
            Field::ThemeNew => text::BUTTON_THEME_NEW.to_owned(),
            Field::ThemeEdit => text::BUTTON_THEME_EDIT.to_owned(),
            Field::ThemeDelete => text::BUTTON_THEME_DELETE.to_owned(),
            Field::ThemeDone => text::BUTTON_THEME_DONE.to_owned(),
            Field::ThemeBack => text::SHORTCUT_BACK.to_owned(),
            Field::ThemeBase => self.theme_base_label(),
            Field::Themes => self
                .theme_list()
                .get(self.theme_selected())
                .map(|theme| theme.name.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Selects one theme of the list, as the mouse does.
    pub fn select_theme(&mut self, id: &str) {
        self.themes.select(id);
        self.focus = Field::Themes;
    }

    /// Puts the focus on one color of the editor, as the mouse does.
    pub fn focus_theme_color(&mut self, role: usize) {
        if let Some(editor) = self.themes.editor_mut() {
            editor.cursor = role;
        }
        self.focus = Field::ThemeColors;
    }

    /// Moves the focused color of the editor, as the scroll wheel does.
    pub fn scroll_theme_color(&mut self, delta: isize) {
        self.themes.move_cursor(delta);
    }

    /// True when either saved scope, the current draft, or a connection refers
    /// to the theme. A shadowed global reference still needs its file.
    fn theme_used(&self, id: &str) -> bool {
        let chosen = |theme: &Option<ThemeName>| matches!(theme, Some(ThemeName::Custom(other)) if other == id);
        let draft = self.draft();
        let other = match self.scope {
            // The draft replaces the global value, and a project value still wins.
            Scope::Global => self
                .project
                .as_ref()
                .and_then(|project| project.theme.clone()),
            // The saved global choice remains a reference even when overridden.
            Scope::Project => self.global.theme.clone(),
        };
        self.themes.in_use(id) || chosen(&draft.theme) || chosen(&other)
    }

    /// Starts the editor on a copy of the theme that the scope chooses.
    pub(super) fn start_new_theme(&mut self) {
        let (base, scheme) = self.themes.base_of(self.saved_theme().as_ref());
        self.themes.start_new(base, scheme);
        self.focus = Field::ThemeName;
        self.note = None;
    }

    pub(super) fn start_edit_theme(&mut self) {
        match self.themes.start_edit() {
            Ok(()) => {
                self.focus = Field::ThemeName;
                self.note = None;
            }
            Err(message) => self.note = Some(message),
        }
    }

    /// Takes the editor draft into the changes of the dialog.
    pub(super) fn finish_theme(&mut self) {
        let scope = self.scope;
        match self.themes.finish(scope) {
            Ok(Some(warning)) => {
                self.focus = Field::Themes;
                self.note = Some(warning);
            }
            Ok(None) => self.focus = Field::Themes,
            Err(message) => self.note = Some(message),
        }
    }

    pub(super) fn delete_theme(&mut self) {
        let id = self
            .theme_list()
            .get(self.theme_selected())
            .map(|theme| theme.id.clone());
        match id {
            Some(id) if self.theme_used(&id) => {
                self.note = Some(text::THEME_IN_USE.to_owned());
            }
            Some(_) => {
                if let Err(message) = self.themes.delete() {
                    self.note = Some(message);
                }
            }
            None => self.note = Some(text::THEME_NONE.to_owned()),
        }
    }

    /// Keys of the theme list and of the theme editor. It returns true when the
    /// key belongs here.
    pub(super) fn theme_key(&mut self, key: KeyEvent, vim: bool) -> bool {
        if self.themes.is_editing() {
            if key.code == KeyCode::Esc {
                self.themes.cancel();
                self.focus = Field::Themes;
                self.note = None;
                return true;
            }
            if self.focus == Field::ThemeColors {
                match key.code {
                    KeyCode::Up => self.themes.move_cursor(-1),
                    KeyCode::Down => self.themes.move_cursor(1),
                    KeyCode::Char('k') if vim => self.themes.move_cursor(-1),
                    KeyCode::Char('j') if vim => self.themes.move_cursor(1),
                    KeyCode::PageUp => self.themes.move_cursor(-8),
                    KeyCode::PageDown => self.themes.move_cursor(8),
                    KeyCode::Home => self.themes.move_cursor(isize::MIN),
                    KeyCode::End => self.themes.move_cursor(isize::MAX),
                    _ => return false,
                }
                return true;
            }
            if key.code == KeyCode::Enter && self.focus == Field::ThemeDone {
                self.finish_theme();
                return true;
            }
            if key.code == KeyCode::Enter && self.focus == Field::ThemeBack {
                self.themes.cancel();
                self.focus = Field::Themes;
                self.note = None;
                return true;
            }
            return false;
        }
        if self.focus == Field::Themes {
            match key.code {
                KeyCode::Up => self.themes.move_selection(-1),
                KeyCode::Down => self.themes.move_selection(1),
                KeyCode::Char('k') if vim => self.themes.move_selection(-1),
                KeyCode::Char('j') if vim => self.themes.move_selection(1),
                KeyCode::Enter => self.start_edit_theme(),
                _ => return false,
            }
            return true;
        }
        match self.focus {
            Field::ThemeNew if key.code == KeyCode::Enter => {
                self.start_new_theme();
                true
            }
            Field::ThemeEdit if key.code == KeyCode::Enter => {
                self.start_edit_theme();
                true
            }
            Field::ThemeDelete if key.code == KeyCode::Enter => {
                self.delete_theme();
                true
            }
            _ => false,
        }
    }

    /// A click on the buttons of the themes.
    pub(super) fn theme_press(&mut self) -> bool {
        if self.themes.is_editing() {
            match self.focus {
                Field::ThemeDone => self.finish_theme(),
                Field::ThemeBack => {
                    self.themes.cancel();
                    self.focus = Field::Themes;
                    self.note = None;
                }
                _ => return false,
            }
            return true;
        }
        match self.focus {
            Field::Themes => self.start_edit_theme(),
            Field::ThemeNew => self.start_new_theme(),
            Field::ThemeEdit => self.start_edit_theme(),
            Field::ThemeDelete => self.delete_theme(),
            _ => return false,
        }
        true
    }
}
