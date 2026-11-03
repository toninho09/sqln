//! The tabs of the last session: the list at the exit and the reopening at the start.

use super::App;
use super::tables::Tab;
use crate::catalog::RelationRef;
use crate::exec::ConsoleId;
use crate::input::Action;
use crate::store::Scope;
use crate::store::scripts::{self, ScriptEntry};
use crate::store::workspace::{self, OpenTab, OpenTabs};
use crate::ui::text;

/// A tab that came back. The identity stays valid when the first console goes.
enum Reopened {
    Console(ConsoleId),
    Table(usize),
}

impl App {
    /// Writes the list of tabs. A console without a script has nothing to reopen.
    pub(super) fn save_open_tabs(&mut self) {
        if !self
            .persist
            .as_ref()
            .is_some_and(|persist| persist.tabs_known)
        {
            return;
        }
        let Some(root) = self.layout_root() else {
            return;
        };
        let active = self.active_tab();
        let mut saved = OpenTabs::default();
        for (index, tab) in self.tab_order().into_iter().enumerate() {
            let Some(item) = self.open_tab(tab) else {
                continue;
            };
            if index == active {
                saved.active = Some(saved.tabs.len());
            }
            saved.tabs.push(item);
        }
        let _ = workspace::update(&root, |workspace| workspace.open_tabs = Some(saved));
    }

    fn open_tab(&self, tab: Tab) -> Option<OpenTab> {
        match tab {
            Tab::Console(index) => {
                let file = self.consoles[index].file.as_ref()?;
                Some(OpenTab::Console {
                    scope: file.scope,
                    id: file.meta.id.clone(),
                })
            }
            // A structure or a diagram costs one key to open, so it does not come back.
            Tab::Table(index) if self.tables[index].structure.is_some() => None,
            Tab::Diagram(_) => None,
            Tab::Table(index) => {
                let tab = &self.tables[index];
                let key = self.connection(tab.connection)?.key.clone()?;
                Some(OpenTab::Table {
                    scope: key.scope,
                    connection: key.id,
                    schema: tab.relation.schema.clone(),
                    relation: tab.relation.relation.clone(),
                    view: tab.is_view,
                })
            }
        }
    }

    /// Opens the tabs of the last session. No tab runs SQL or opens a connection.
    pub(super) fn reopen_tabs(&mut self) {
        let Some(persist) = &mut self.persist else {
            return;
        };
        // An invalid configuration has no connections, so the list waits for a later start.
        if persist.error.is_some() {
            persist.tabs_known = false;
            return;
        }
        if !persist.reopen_tabs {
            return;
        }
        let Some(saved) = self
            .layout_root()
            .and_then(|root| workspace::load(&root).open_tabs)
        else {
            return;
        };
        let scripts = self.saved_scripts();
        let first = self.console().id;
        let mut any = false;
        let mut on_screen = None;
        for (index, tab) in saved.tabs.into_iter().enumerate() {
            let Some(tab) = self.reopen_tab(tab, &scripts) else {
                continue;
            };
            any = true;
            if saved.active == Some(index) {
                on_screen = Some(tab);
            }
        }
        if !any {
            return;
        }
        // The empty console of the start gives its place to the tabs.
        self.remove_console(first);
        match on_screen {
            Some(Reopened::Console(id)) => {
                if let Some(index) = self.console_index(id) {
                    self.active = index;
                    self.table_active = None;
                }
            }
            Some(Reopened::Table(index)) => self.table_active = Some(index),
            None => {}
        }
    }

    fn saved_scripts(&self) -> Vec<ScriptEntry> {
        let scopes = [Scope::Global, Scope::Project];
        let roots = scopes.map(|scope| self.persist_root(scope).map(|root| (scope, root)));
        roots
            .into_iter()
            .flatten()
            .flat_map(|(scope, root)| scripts::list(&root, scope))
            .collect()
    }

    fn reopen_tab(&mut self, tab: OpenTab, scripts: &[ScriptEntry]) -> Option<Reopened> {
        match tab {
            OpenTab::Console { scope, id } => {
                let entry = scripts
                    .iter()
                    .find(|entry| entry.scope == scope && entry.meta.id == id)?;
                self.reopen_console(entry)
            }
            OpenTab::Table {
                scope,
                connection,
                schema,
                relation,
                view,
            } => {
                let connection = self.connection_by_key(scope, &connection)?.id;
                let id = self.manager.as_mut()?.open_console(connection)?;
                let relation = RelationRef { schema, relation };
                let mut tab = self.new_table_tab(id, connection, relation, view);
                tab.wait_for_use(text::table_pending(
                    self.shown_key(Action::GridOpenValue).as_deref(),
                ));
                self.tables.push(tab);
                Some(Reopened::Table(self.tables.len() - 1))
            }
        }
    }

    /// Opens a saved console. A console without its connection stays in the list of
    /// scripts without a connection.
    fn reopen_console(&mut self, entry: &ScriptEntry) -> Option<Reopened> {
        self.connection_by_key(entry.scope, &entry.meta.connection_id)?;
        match self.load_script(entry) {
            Ok(index) => Some(Reopened::Console(self.consoles[index].id)),
            Err(message) => {
                self.status = Some(message);
                // The tab can open at a later start, so the list stays in the file.
                if let Some(persist) = &mut self.persist {
                    persist.tabs_known = false;
                }
                None
            }
        }
    }
}
