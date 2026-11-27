//! The key list and the guide.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Screen};
use crate::help::{GuideTopic, Help};
use crate::input::{Action, Grid, Region};
use crate::ui::text;
use crate::ui::which_key::KeyGroup;

impl App {
    pub(super) fn on_help_key(&mut self, key: KeyEvent) {
        let Some(help) = &mut self.modals.help else {
            return;
        };
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            let contents = help.text();
            self.copy(&contents);
        } else if help.on_key(key) {
            self.modals.help = None;
        }
    }

    pub(super) fn open_keys(&mut self) {
        self.modals.help = Some(self.help());
    }

    /// Returns the help of the current context: the guide and the key groups.
    pub(super) fn help(&self) -> Help {
        let in_table = self.table_index().is_some() || self.compare_index().is_some();
        // The explorer is the only panel of the home screen.
        let panel = if self.navigator_focused || self.home() {
            Some(Region::Navigator)
        } else if self.context_region() == Some(Region::Review) {
            Some(Region::Review)
        } else {
            self.panel()
        };
        let (region, name) = match panel {
            Some(Region::Navigator) => (Some(Region::Navigator), text::REGION_NAVIGATOR),
            Some(Region::Diagram) => (Some(Region::Diagram), text::REGION_DIAGRAM),
            Some(Region::Review) => (Some(Region::Review), text::REGION_REVIEW),
            Some(_) if self.compare_index().is_some() => (Some(Region::Grid), text::REGION_COMPARE),
            Some(_) if in_table => (Some(Region::Grid), text::REGION_TABLE),
            Some(_) => (Some(Region::Grid), text::REGION_GRID),
            None if in_table => (None, text::REGION_FILTER),
            None => (None, text::REGION_EDITOR),
        };
        let keys = match region {
            Some(region) => self.help_keys(region),
            None => {
                let keys = if in_table {
                    text::FILTER_KEYS
                } else {
                    text::EDITOR_KEYS
                };
                keys.iter()
                    .map(|(key, label)| ((*key).to_owned(), *label))
                    .collect()
            }
        };
        let mut keys = keys;
        if region.is_none() && !in_table {
            keys.splice(0..0, self.help_keys(Region::Editor));
        }
        let mut groups = vec![KeyGroup { title: name, keys }];
        for (action, keys, label) in self.help_actions(Region::Global) {
            use Action::*;
            let title = match action {
                Run | RunScript | RunSubquery | Explain | ExplainAnalyze | Stop | FormatSql
                | SqlDiagnostics | ToggleComment | OpenHistory => text::GROUP_RUN,
                ToggleMode | Commit | Rollback => text::GROUP_TRANSACTION,
                ChooseSchema | ChangeConnection | OpenSessions | OpenLocks | OpenDiagram
                | OpenCompare | FollowKey => text::GROUP_CONNECTION,
                NextTab | PreviousTab | CloseTab | RenameConsole => text::GROUP_TABS,
                FocusLeft | FocusRight | FocusUp | FocusDown | ToggleNavigator
                | ExplorerCommands | ContextCommands => text::GROUP_PANELS,
                _ => text::GROUP_APPLICATION,
            };
            if let Some(group) = groups.iter_mut().find(|group| group.title == title) {
                group.keys.push((keys, label));
            } else {
                groups.push(KeyGroup {
                    title,
                    keys: vec![(keys, label)],
                });
            }
        }
        let mode = if self.vim() {
            text::MODE_VIM
        } else {
            text::MODE_CONVENTIONAL
        };
        Help::new(text::keys_title(mode, name), self.guide_topics(), groups)
    }

    /// Returns the guide with the effective key of each step.
    pub(super) fn guide_topics(&self) -> Vec<GuideTopic> {
        text::GUIDE
            .iter()
            .map(|topic| GuideTopic {
                group: topic.group,
                name: topic.name,
                title: topic.title,
                intro: topic.intro,
                steps: topic
                    .steps
                    .iter()
                    .map(|(action, label)| (self.guide_key(*action), *label))
                    .collect(),
                examples: topic.examples,
            })
            .collect()
    }

    /// Returns the effective key of a guide step. An explorer command starts with the
    /// focus key. A command without a key shows the palette path.
    pub(super) fn guide_key(&self, action: Action) -> String {
        let Some(key) = self.shown_key(action) else {
            let palette = self.shown_key(Action::OpenPalette).unwrap_or_default();
            return format!("{palette} → {}", self.command_label(action));
        };
        if action.region() != Region::Navigator {
            return key;
        }
        match self.shown_key(self.explorer_entry()) {
            Some(focus) => format!("{focus} {key}"),
            None => key,
        }
    }

    pub(super) fn help_keys(&self, region: Region) -> Vec<(String, &'static str)> {
        self.help_actions(region)
            .into_iter()
            .map(|(_, key, label)| (key, label))
            .collect()
    }

    pub(super) fn help_actions(&self, region: Region) -> Vec<(Action, String, &'static str)> {
        let grid = match self.tabs.screen {
            Screen::Table(_) => Grid::Table,
            Screen::Compare(_) => Grid::Compare,
            Screen::Console | Screen::Diagram(_) => Grid::Console,
        };
        let in_table = grid != Grid::Console;
        let sections = self.table().is_some_and(|tab| tab.section().is_some());
        self.keymap
            .actions(region)
            .filter(|action| {
                // Each grid shows only the keys that it has. A key of the table shows only
                // when it has an effect now.
                action.on_grid(grid) && (!action.is_table_only() || self.command_available(*action))
            })
            .filter_map(|action| {
                let keys = self.keymap.keys(action);
                let label = text::grid_action_text(action, in_table, sections);
                (!keys.is_empty()).then(|| (action, keys.join(", "), label))
            })
            .collect()
    }
}
