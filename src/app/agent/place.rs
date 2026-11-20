//! Placing agent SQL in consoles and agent tabs.

use super::*;

impl App {
    /// Opens the agent tab of a session without a connection, after the tab row's other
    /// tabs. The focus and the active tab stay.
    pub(in crate::app) fn open_agent_tab(&mut self, session: &str) {
        let mut console = self.unbound_console(self.fallback_dialect);
        console.name =
            crate::ui::text::agent_console_name(&self.agent.client(session), short(session));
        console.focus = crate::console::Focus::Results;
        self.agent.owned.insert(console.id, session.to_owned());
        self.tabs.consoles.push(console);
    }

    /// Keeps one agent tab for each session with access. A session without a tab gets one.
    pub(in crate::app) fn ensure_agent_tabs(&mut self) {
        if self.exiting {
            return;
        }
        let sessions: Vec<_> = self
            .agent
            .allowed_ids()
            .filter(|session| !self.agent.owned.values().any(|owner| owner == session))
            .collect();
        for session in sessions {
            self.open_agent_tab(&session);
        }
    }

    /// Returns true when closing this agent tab leaves its connected session without a tab.
    pub(in crate::app) fn last_agent_tab(&self, console: ConsoleId) -> Option<String> {
        let owner = self.agent.owned.get(&console)?;
        self.agent.level(owner)?;
        let tabs = self
            .agent
            .owned
            .values()
            .filter(|item| *item == owner)
            .count();
        (tabs == 1 && self.agent.tab_session(console).is_some()).then(|| owner.clone())
    }

    /// Places SQL in the editor of the session's agent tab for the user. Nothing executes.
    /// SQL that the user changed stays: the new SQL then waits for a review.
    pub(super) fn agent_place(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: Option<String>,
        sql: &str,
    ) -> Reply {
        if sql.trim().is_empty() {
            return Err(Error::new(
                "invalid_request",
                "Send the SQL that the user must review.",
            ));
        }
        if sql.len() > agent::MAX_FRAME {
            return Err(Error::new(
                "request_too_large",
                "Send at most 8 MiB of SQL.",
            ));
        }
        let namespace = self
            .connection(connection)
            .ok_or_else(missing)?
            .capabilities
            .namespace
            != crate::connector::Namespace::None;
        let console = self.agent_console(session, connection, None)?;
        let index = self.console_index(console).ok_or_else(missing)?;
        // A running task keeps the schema of its session.
        if namespace
            && schema.is_some()
            && !self.tabs.consoles[index].is_busy()
            && !self.tabs.consoles[index].in_transaction()
        {
            self.tabs.consoles[index].schema = schema;
        }
        let text = sql.replace("\r\n", "\n").replace('\r', "\n");
        let editor = &self.tabs.consoles[index].editor;
        let revision = editor.revision();
        let edit = editor.agent_replace_all(&text);
        if !editor.is_blank() && self.agent.placed.get(&console) != Some(&revision) {
            return self.agent_edit(
                session,
                console,
                revision,
                &[edit],
                agent::EditMode::Propose,
            );
        }
        let editor = &mut self.tabs.consoles[index].editor;
        editor.apply_agent_edits(revision, &[edit])?;
        // New SQL shows from its first line.
        editor.set_cursor(edtui::Index2::new(0, 0));
        editor.set_scroll(0, 0);
        self.agent_sql_placed(console);
        let client = self.agent.client(session);
        let run = self.shown_key(crate::input::Action::Run);
        self.tabs.consoles[index].messages.note(
            crate::console::MessageLevel::Info,
            &crate::ui::text::agent_placed_note(&client, run.as_deref()),
            None,
        );
        self.status = Some(crate::ui::text::agent_placed(&client));
        Ok(json!({
            "console_id": console.to_string(),
            "revision": self.tabs.consoles[index].editor.revision(),
            "status": "placed",
        }))
    }

    /// Records SQL that the agent placed in its tab, and marks the tab until the user sees it.
    pub(in crate::app) fn agent_sql_placed(&mut self, console: ConsoleId) {
        let Some(index) = self.console_index(console) else {
            return;
        };
        self.agent
            .placed
            .insert(console, self.tabs.consoles[index].editor.revision());
        if !(index == self.tabs.active && self.console_on_screen()) {
            self.agent.fresh.insert(console);
        }
        self.refresh_tree_consoles();
    }

    /// Removes the mark of SQL that the user did not see, when the agent tab is on screen.
    pub(in crate::app) fn see_agent_sql(&mut self) {
        let id = self.console().id;
        if self.console_on_screen() && self.agent.fresh.remove(&id) {
            self.refresh_tree_consoles();
        }
    }

    /// Copies the SQL of the agent tab to a new user console, which takes the focus.
    pub(in crate::app) fn agent_sql_to_console(&mut self) {
        let console = self.console();
        let Some(connection) = console.connection else {
            return;
        };
        let (schema, text) = (console.schema.clone(), console.editor.text());
        self.open_text_console(connection, schema, &text);
    }

    /// Empties the SQL of the agent tab as one undo step. The tab then shows its results only.
    pub(in crate::app) fn agent_sql_clear(&mut self) {
        let editor = &self.console().editor;
        let (revision, edit) = (editor.revision(), editor.agent_replace_all(""));
        if self
            .console_mut()
            .editor
            .apply_agent_edits(revision, &[edit])
            .is_ok()
        {
            let undo = self.shown_key(crate::input::Action::EditorUndo);
            self.status = Some(crate::ui::text::agent_sql_cleared(undo.as_deref()));
        }
    }
}
