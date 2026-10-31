//! The key map: the action of each key in each region.
//!
//! A binding is one key, or two keys in sequence, such as `ctrl+w e`. The editor keys
//! stay in `editor`, because the EdTUI handler owns the Vim mode.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::editor::KeyboardMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Chord {
    pub fn from_event(key: KeyEvent) -> Self {
        let mut modifiers =
            key.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        // A character holds its case, so Shift adds nothing.
        if matches!(key.code, KeyCode::Char(_)) {
            modifiers -= KeyModifiers::SHIFT;
        }
        Self { code: key.code, modifiers }
    }

    /// Returns true for `Ctrl+W`, the key before the commands of the application.
    pub fn is_prefix(self) -> bool {
        self.code == KeyCode::Char('w') && self.modifiers == KeyModifiers::CONTROL
    }

    /// Returns the same key without Shift, when the key has Shift.
    pub fn without_shift(self) -> Option<Self> {
        self.modifiers.contains(KeyModifiers::SHIFT).then(|| Self {
            modifiers: self.modifiers - KeyModifiers::SHIFT,
            ..self
        })
    }

    /// Reads a key such as `ctrl+enter`, `shift+f5`, `alt+up`, or `<`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (modifier_text, name) = match text.rsplit_once('+') {
            Some((start, "")) => (start.strip_suffix('+').unwrap_or(start), "+"),
            Some(parts) => parts,
            None => ("", text),
        };
        let mut modifiers = KeyModifiers::NONE;
        for modifier in modifier_text.split('+').filter(|modifier| !modifier.is_empty()) {
            modifiers |= match modifier.to_ascii_lowercase().as_str() {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => return Err(format!("Unknown key: {text}")),
            };
        }
        let code = key_code(name).ok_or_else(|| format!("Unknown key: {text}"))?;
        Ok(Self::from_event(KeyEvent::new(code, modifiers)))
    }
}

fn key_code(name: &str) -> Option<KeyCode> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(c));
    }
    let code = match name.to_ascii_lowercase().as_str() {
        "enter" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "space" => KeyCode::Char(' '),
        other => KeyCode::F(other.strip_prefix('f')?.parse().ok()?),
    };
    Some(code)
}

/// The place where a key has its meaning. The panels come before `Global`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Global,
    Navigator,
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Run,
    RunScript,
    Stop,
    ToggleMode,
    Commit,
    Rollback,
    Help,
    CloseTab,
    Quit,
    NextTab,
    PreviousTab,
    ToggleNavigator,
    ChooseSchema,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    NavigatorUp,
    NavigatorDown,
    NavigatorCollapse,
    NavigatorExpand,
    NavigatorOpen,
    NavigatorNewConsole,
    NavigatorRefresh,
    NavigatorDisconnect,
    NavigatorEdit,
    NavigatorDelete,
    NavigatorAdd,
    GridUp,
    GridDown,
    GridLeft,
    GridRight,
    GridPageUp,
    GridPageDown,
    GridFirstRow,
    GridLastRow,
    GridFirstColumn,
    GridLastColumn,
    GridSelectAll,
    GridToggleSelection,
    GridClearSelection,
    GridCopy,
    GridOpenValue,
    GridFind,
    GridNarrow,
    GridWiden,
    GridAutoWidth,
    GridCloseResult,
    GridPreviousResult,
    GridNextResult,
    GridFocusFilter,
    GridFocusOrder,
    GridSortColumn,
    GridLoadMore,
}

impl Action {
    /// Returns true for an action that only the grid of a table has.
    pub fn is_table_only(self) -> bool {
        matches!(
            self,
            Action::GridFocusFilter
                | Action::GridFocusOrder
                | Action::GridSortColumn
                | Action::GridLoadMore
        )
    }

    /// Returns true for an action that only the results of a console have.
    pub fn is_console_only(self) -> bool {
        matches!(
            self,
            Action::GridCloseResult | Action::GridPreviousResult | Action::GridNextResult
        )
    }

    /// Returns true for a move that Shift changes into a selection.
    pub fn extends_selection(self) -> bool {
        matches!(
            self,
            Action::GridUp
                | Action::GridDown
                | Action::GridLeft
                | Action::GridRight
                | Action::GridPageUp
                | Action::GridPageDown
                | Action::GridFirstRow
                | Action::GridLastRow
                | Action::GridFirstColumn
                | Action::GridLastColumn
        )
    }
}

struct Item {
    action: Action,
    region: Region,
    /// The name of the action in `config.json`.
    name: &'static str,
    keys: &'static [&'static str],
    /// The keys of the Vim mode, in addition to `keys`.
    vim_keys: &'static [&'static str],
    /// True when `keys` applies only to the conventional mode.
    conventional_only: bool,
}

const fn both(
    action: Action,
    region: Region,
    name: &'static str,
    keys: &'static [&'static str],
    vim_keys: &'static [&'static str],
) -> Item {
    Item { action, region, name, keys, vim_keys, conventional_only: false }
}

const DEFAULTS: &[Item] = &[
    both(Action::Run, Region::Global, "run", &["ctrl+enter", "f5", "ctrl+w e"], &[]),
    both(Action::RunScript, Region::Global, "runScript", &["shift+enter", "shift+f5", "ctrl+w s"], &[]),
    both(Action::Stop, Region::Global, "stop", &["ctrl+w x"], &[]),
    both(Action::ToggleMode, Region::Global, "toggleMode", &["ctrl+w m"], &[]),
    both(Action::Commit, Region::Global, "commit", &["ctrl+w C"], &[]),
    both(Action::Rollback, Region::Global, "rollback", &["ctrl+w R"], &[]),
    both(Action::Help, Region::Global, "help", &["f1", "ctrl+w ?"], &[]),
    both(Action::CloseTab, Region::Global, "closeTab", &["ctrl+w c"], &[]),
    both(Action::Quit, Region::Global, "quit", &["ctrl+w q"], &[]),
    both(Action::NextTab, Region::Global, "nextTab", &["ctrl+w n"], &[]),
    both(Action::PreviousTab, Region::Global, "previousTab", &["ctrl+w p"], &[]),
    both(Action::ToggleNavigator, Region::Global, "toggleNavigator", &["ctrl+w b"], &[]),
    both(Action::ChooseSchema, Region::Global, "chooseSchema", &["ctrl+w d"], &[]),
    both(Action::FocusLeft, Region::Global, "focusLeft", &["ctrl+w h", "ctrl+w left"], &[]),
    both(Action::FocusRight, Region::Global, "focusRight", &["ctrl+w l", "ctrl+w right"], &[]),
    both(Action::FocusUp, Region::Global, "focusUp", &["ctrl+w k", "ctrl+w up"], &[]),
    both(Action::FocusDown, Region::Global, "focusDown", &["ctrl+w j", "ctrl+w down"], &[]),
    both(Action::NavigatorUp, Region::Navigator, "navigatorUp", &["up"], &["k"]),
    both(Action::NavigatorDown, Region::Navigator, "navigatorDown", &["down"], &["j"]),
    both(Action::NavigatorCollapse, Region::Navigator, "navigatorCollapse", &["left"], &["h"]),
    both(Action::NavigatorExpand, Region::Navigator, "navigatorExpand", &["right"], &["l"]),
    both(Action::NavigatorOpen, Region::Navigator, "navigatorOpen", &["enter"], &[]),
    both(Action::NavigatorNewConsole, Region::Navigator, "newConsole", &["n"], &[]),
    both(Action::NavigatorRefresh, Region::Navigator, "refreshCatalog", &["r"], &[]),
    both(Action::NavigatorDisconnect, Region::Navigator, "disconnect", &["D"], &[]),
    both(Action::NavigatorEdit, Region::Navigator, "editConnection", &["e"], &[]),
    Item {
        action: Action::NavigatorDelete,
        region: Region::Navigator,
        name: "deleteConnection",
        keys: &["delete"],
        vim_keys: &["x"],
        conventional_only: true,
    },
    both(Action::NavigatorAdd, Region::Navigator, "addConnection", &["a"], &[]),
    both(Action::GridUp, Region::Grid, "gridUp", &["up"], &["k"]),
    both(Action::GridDown, Region::Grid, "gridDown", &["down"], &["j"]),
    both(Action::GridLeft, Region::Grid, "gridLeft", &["left"], &["h"]),
    both(Action::GridRight, Region::Grid, "gridRight", &["right"], &["l"]),
    both(Action::GridPageUp, Region::Grid, "gridPageUp", &["pageup"], &[]),
    both(Action::GridPageDown, Region::Grid, "gridPageDown", &["pagedown"], &[]),
    both(Action::GridFirstRow, Region::Grid, "gridFirstRow", &["alt+up"], &["g g"]),
    both(Action::GridLastRow, Region::Grid, "gridLastRow", &["alt+down"], &["G"]),
    both(Action::GridFirstColumn, Region::Grid, "gridFirstColumn", &["alt+left"], &["0"]),
    both(Action::GridLastColumn, Region::Grid, "gridLastColumn", &["alt+right"], &["$"]),
    both(Action::GridSelectAll, Region::Grid, "gridSelectAll", &["ctrl+a"], &[]),
    both(Action::GridToggleSelection, Region::Grid, "gridToggleSelection", &[], &["v"]),
    both(Action::GridClearSelection, Region::Grid, "gridClearSelection", &["esc"], &[]),
    both(Action::GridCopy, Region::Grid, "gridCopy", &["ctrl+c"], &["y"]),
    both(Action::GridOpenValue, Region::Grid, "gridOpenValue", &["enter"], &[]),
    both(Action::GridFind, Region::Grid, "gridFind", &["ctrl+f"], &["/"]),
    both(Action::GridNarrow, Region::Grid, "gridNarrow", &["<"], &[]),
    both(Action::GridWiden, Region::Grid, "gridWiden", &[">"], &[]),
    both(Action::GridAutoWidth, Region::Grid, "gridAutoWidth", &["="], &[]),
    both(Action::GridCloseResult, Region::Grid, "gridCloseResult", &["x", "delete"], &[]),
    both(Action::GridPreviousResult, Region::Grid, "gridPreviousResult", &["["], &[]),
    both(Action::GridNextResult, Region::Grid, "gridNextResult", &["]"], &[]),
    both(Action::GridFocusFilter, Region::Grid, "focusFilter", &["f"], &[]),
    both(Action::GridFocusOrder, Region::Grid, "focusOrder", &["o"], &[]),
    both(Action::GridSortColumn, Region::Grid, "sortColumn", &["s"], &[]),
    both(Action::GridLoadMore, Region::Grid, "loadMore", &["m"], &[]),
];

/// The answer of the map to one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Action(Action),
    /// The key starts a sequence, and the next key completes it.
    Sequence,
    None,
}

#[derive(Debug)]
pub struct Keymap {
    single: HashMap<(Region, Chord), Action>,
    double: HashMap<(Region, Chord, Chord), Action>,
    starts: HashMap<(Region, Chord), Action>,
    keys: HashMap<Action, Vec<String>>,
}

impl Keymap {
    pub fn new(mode: KeyboardMode) -> Self {
        Self::with_overrides(mode, &HashMap::new()).expect("the default keys have no conflict")
    }

    /// Applies the `keymap` part of the configuration. A list replaces the default
    /// keys of its action, and an empty list removes them.
    pub fn with_overrides(
        mode: KeyboardMode,
        overrides: &HashMap<String, Vec<String>>,
    ) -> Result<Self, String> {
        if let Some(name) = overrides.keys().find(|name| !DEFAULTS.iter().any(|item| item.name == *name)) {
            return Err(format!("Unknown action: {name}"));
        }
        let mut map = Self {
            single: HashMap::new(),
            double: HashMap::new(),
            starts: HashMap::new(),
            keys: HashMap::new(),
        };
        for item in DEFAULTS {
            let keys: Vec<String> = match overrides.get(item.name) {
                Some(keys) => keys.clone(),
                None => default_keys(item, mode),
            };
            for text in &keys {
                map.bind(item.region, item.action, item.name, text)?;
            }
            map.keys.insert(item.action, keys);
        }
        Ok(map)
    }

    fn bind(&mut self, region: Region, action: Action, name: &str, text: &str) -> Result<(), String> {
        let conflict = || format!("Two actions use the key {text}. One of them is {name}.");
        let chords = text
            .split_whitespace()
            .map(Chord::parse)
            .collect::<Result<Vec<_>, _>>()?;
        match chords.as_slice() {
            [key] => {
                if self.starts.contains_key(&(region, *key)) {
                    return Err(conflict());
                }
                if self.single.insert((region, *key), action).is_some() {
                    return Err(conflict());
                }
            }
            [first, second] => {
                if self.single.contains_key(&(region, *first)) {
                    return Err(conflict());
                }
                if self.double.insert((region, *first, *second), action).is_some() {
                    return Err(conflict());
                }
                self.starts.insert((region, *first), action);
            }
            _ => return Err(format!("Unknown key: {text}")),
        }
        Ok(())
    }

    pub fn lookup(&self, region: Region, key: Chord) -> Lookup {
        if let Some(action) = self.single.get(&(region, key)) {
            return Lookup::Action(*action);
        }
        if self.starts.contains_key(&(region, key)) {
            return Lookup::Sequence;
        }
        Lookup::None
    }

    /// Returns the action of the second key of a sequence.
    pub fn after(&self, region: Region, first: Chord, second: Chord) -> Option<Action> {
        self.double.get(&(region, first, second)).copied()
    }

    /// Returns the keys of an action, as the user writes them.
    pub fn keys(&self, action: Action) -> &[String] {
        self.keys.get(&action).map_or(&[], Vec::as_slice)
    }

    /// Lists the actions of a region in the order of the help screen.
    pub fn actions(&self, region: Region) -> impl Iterator<Item = Action> + '_ {
        DEFAULTS.iter().filter(move |item| item.region == region).map(|item| item.action)
    }
}

fn default_keys(item: &Item, mode: KeyboardMode) -> Vec<String> {
    let vim = mode == KeyboardMode::Vim;
    let base = if vim && item.conventional_only { &[][..] } else { item.keys };
    let extra = if vim { item.vim_keys } else { &[] };
    base.iter().chain(extra).map(|key| (*key).to_owned()).collect()
}
