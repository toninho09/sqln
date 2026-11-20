//! The input events, the execution events, and the steps of the main loop.

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};

use super::modal::Modal;
use super::tables::Screen;
use super::{App, KEY_WAIT, RUN_FRAME, STATUS_TIME, WORK_SLICE};
use crate::compare_tab::CompareTab;
use crate::console::{Console, Focus};
use crate::diagram_tab::DiagramTab;
use crate::exec::{ConsoleId, ExecEvent};
use crate::table::{TableFocus, TableTab};
use crate::ui::text;

impl App {
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
    pub(super) fn keep_agent_focus(&mut self) {
        let Some(console) = self.tabs.consoles.get(self.tabs.active) else {
            return;
        };
        if console.focus != Focus::Results && self.agent_tab_without_sql(console.id) {
            self.tabs.consoles[self.tabs.active].focus = Focus::Results;
        }
        self.see_agent_sql();
    }

    /// Returns true for an agent tab that holds no SQL for the user. It shows no editor.
    pub(super) fn agent_tab_without_sql(&self, console: ConsoleId) -> bool {
        self.agent.owns(console)
            && self
                .console_index(console)
                .is_some_and(|index| self.tabs.consoles[index].editor.is_blank())
    }

    /// Moves the clock of the screen. It returns true when a run or the age of an open
    /// transaction needs a new frame.
    pub(super) fn advance_clock(&mut self, now: Instant) -> bool {
        self.time.now = now;
        for console in &mut self.tabs.consoles {
            console.sample_progress(now);
        }
        for table in &mut self.tabs.tables {
            table.sample_progress(now);
        }
        let time = now.saturating_duration_since(self.time.started);
        let frame = usize::try_from(time.as_millis() / RUN_FRAME.as_millis()).unwrap_or(0);
        // An agent task updates the time of its card, and the card appears after a delay.
        let running = self.ai.busy()
            || self.agent.active_tasks() != 0
            || self.tabs.consoles.iter().any(Console::is_busy)
            || self.tabs.tables.iter().any(TableTab::is_busy)
            || self.tabs.diagrams.iter().any(DiagramTab::is_busy)
            || self.tabs.compares.iter().any(CompareTab::is_busy);
        let changed = running && frame != self.time.run_frame;
        self.time.run_frame = frame;
        let minute = self
            .console()
            .transaction_age(now)
            .map(|age| age.as_secs() / 60);
        let aged = minute != self.time.transaction_minute;
        self.time.transaction_minute = minute;
        changed || aged
    }

    pub(super) fn clear_status(&mut self) {
        self.status = None;
        self.status_shown = None;
    }

    /// Starts the time of a new message, and removes a message at the end of its time.
    /// It returns true when a message goes.
    pub(super) fn expire_status(&mut self, now: Instant) -> bool {
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

    /// Returns true when a window blocks the screen and the background work. The
    /// connection form does not block them.
    pub(super) fn modal_is_open(&self) -> bool {
        self.ai.modal() || self.top_modal().is_some_and(|modal| modal != Modal::Form)
    }

    pub(super) fn route_event(&mut self, event: Event) {
        if self.modals.dialogs.is_empty() && self.auth.prompt.is_none() {
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
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) => match self.top_modal() {
                Some(modal) if modal != Modal::Form => self.on_modal_paste(modal, &text),
                _ if !self.modal_is_open() => self.on_paste(&text),
                _ => {}
            },
            Event::Mouse(mouse) => match self.top_modal() {
                Some(modal) if modal != Modal::Form => self.on_modal_mouse(modal, mouse),
                _ if !self.modal_is_open() => self.on_mouse(mouse),
                _ => {}
            },
            _ => {}
        }
    }

    /// Gives a paste to a window over the screen. A window without text ignores it.
    pub(super) fn on_modal_paste(&mut self, modal: Modal, text: &str) {
        match modal {
            Modal::Password => {
                if let Some(prompt) = &mut self.auth.prompt {
                    prompt.paste(text);
                }
            }
            Modal::Rename => {
                if let Some(prompt) = &mut self.modals.rename {
                    prompt.paste(text);
                }
            }
            Modal::Browser => {
                if let Some(browser) = &mut self.modals.browser {
                    browser.paste(text);
                }
            }
            Modal::Settings => {
                if let Some(form) = &mut self.modals.settings_form {
                    form.paste(text);
                }
            }
            Modal::Export => {
                if let Some(form) = &mut self.modals.export_form {
                    form.paste(text);
                }
            }
            Modal::Edit => {
                if let Some(crate::edit_form::EditForm {
                    body: crate::edit_form::Body::Cell { editor, .. },
                    ..
                }) = &mut self.modals.edit_form
                {
                    editor.on_paste(text);
                }
            }
            Modal::Import => {
                if let Some(form) = &mut self.modals.import_form {
                    form.paste(text);
                }
            }
            Modal::Compare => {
                if let Some(form) = &mut self.modals.compare_form {
                    form.paste(text);
                }
            }
            Modal::Palette => {
                if let Some(palette) = &mut self.modals.palette {
                    palette.paste(text);
                }
            }
            Modal::History => {
                if let Some(list) = &mut self.modals.history {
                    list.paste(text);
                }
            }
            Modal::Overlay => {
                if let Some(overlay) = &mut self.modals.overlay {
                    overlay.contents.paste_search(text);
                }
            }
            Modal::Help => {
                if let Some(help) = &mut self.modals.help {
                    help.paste(text);
                }
            }
            // The screen handles the paste of the connection form.
            Modal::Dialog | Modal::Conflicts | Modal::Picker | Modal::Form => {}
        }
    }

    /// Gives a mouse event to a window over the screen.
    pub(super) fn on_modal_mouse(&mut self, modal: Modal, mouse: MouseEvent) {
        match modal {
            Modal::Dialog => self.on_dialog_mouse(mouse),
            Modal::Password => self.on_password_mouse(mouse),
            Modal::Conflicts => self.on_conflicts_mouse(mouse),
            Modal::Browser => self.on_browser_mouse(mouse),
            Modal::Picker => self.on_picker_mouse(mouse),
            Modal::Palette => self.on_palette_mouse(mouse),
            Modal::History => self.on_history_mouse(mouse),
            Modal::Settings => self.on_settings_mouse(mouse),
            Modal::Export => self.on_export_mouse(mouse),
            Modal::Edit => self.on_edit_mouse(mouse),
            Modal::Import => self.on_import_mouse(mouse),
            Modal::Compare => self.on_compare_mouse(mouse),
            Modal::Overlay => self.on_overlay_mouse(mouse),
            Modal::Help => {
                if let Some(help) = &mut self.modals.help {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => help.scroll = help.scroll.saturating_sub(3),
                        MouseEventKind::ScrollDown => help.scroll = help.scroll.saturating_add(3),
                        MouseEventKind::Down(MouseButton::Left) => {
                            help.click(mouse.column, mouse.row);
                        }
                        _ => {}
                    }
                }
            }
            // The screen handles the mouse of the connection form.
            Modal::Rename | Modal::Form => {}
        }
    }

    pub(super) fn on_paste(&mut self, text: &str) {
        if let Some(form) = &mut self.modals.form {
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
        if self.diagram_index().is_some() || self.home() {
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
        search
            || self
                .modals
                .overlay
                .as_ref()
                .is_some_and(|overlay| overlay.contents.search_has_work())
            || self.exports_have_work()
            || self.analysis_job.is_some()
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
        let probed = self.poll_probes() | self.poll_registry_writes();
        let changed =
            self.has_work() || ticked || authorized || probed || agent_changed || ai_changed;
        if changed {
            let deadline = Instant::now() + WORK_SLICE;
            self.continue_overlay_search(Instant::now() + WORK_SLICE / 2);
            match self.tabs.screen {
                Screen::Compare(index) => {
                    self.tabs.compares[index].result.continue_search(deadline)
                }
                Screen::Table(index) => self.tabs.tables[index].result.continue_search(deadline),
                Screen::Console | Screen::Diagram(_) => {
                    self.console_mut().continue_search(deadline)
                }
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

    pub(super) fn on_exec_event(&mut self, event: ExecEvent) {
        self.note_agent_event(&event);
        match event {
            ExecEvent::AgentMetadata { job, result } => self.on_agent_metadata(&job, result),
            ExecEvent::CommandLog { target, event } => {
                if let Some(console) = self
                    .tabs
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
                notes,
                foreign_keys,
                referencing,
            } => {
                let columns = columns.map(|columns| (columns, notes));
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

    pub(super) fn on_console_event(&mut self, mut event: ExecEvent) {
        if self.on_edit_event(&mut event)
            || self.on_export_event(&event)
            || self.on_import_event(&mut event)
            || self.on_definition_event(&mut event)
        {
            return;
        }
        let Some(id) = event.console() else { return };
        let budget = self.budget.clone();
        if let Some(tab) = self.tabs.compares.iter_mut().find(|tab| tab.owns(id)) {
            tab.on_event(event, &budget);
            self.continue_compare(id);
            return;
        }
        if let Some(tab) = self.tabs.diagrams.iter_mut().find(|tab| tab.id == id) {
            tab.on_event(event, &budget);
            return;
        }
        if let Some(index) = self.tabs.tables.iter().position(|tab| tab.id == id) {
            let finished = matches!(event, ExecEvent::ExecutionFinished { .. });
            if finished {
                let auto = self.tabs.tables[index].auto_execution;
                self.on_auto_execution_end(&event, id, auto);
            }
            self.tabs.tables[index].on_event(event, &budget);
            if finished
                && !self.tabs.tables[index].is_busy()
                && let Some(restore) = self.tabs.tables[index].edit_restore.take()
                && let Some(view) = restore.view_in(&self.tabs.tables[index].result.table)
            {
                self.tabs.tables[index].result.view = view;
            }
            self.finish_session_signal(index);
            self.continue_wanted_import();
            return;
        }
        if let ExecEvent::StatementFinished { key, status, .. } = &event
            && self
                .console_index(id)
                .is_none_or(|index| self.tabs.consoles[index].auto_execution != Some(key.execution))
        {
            self.note_statement(key, status);
        }
        if let Some(index) = self.console_index(id) {
            let auto = self.tabs.consoles[index].auto_execution;
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
