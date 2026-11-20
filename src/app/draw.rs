//! The drawing of the screen.

use edtui::EditorMode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use super::modal::Modal;
use super::tables::Screen;
use super::{App, MIN_HEIGHT, MIN_WIDTH, Overlay, TALL_HEIGHT};
use crate::connection::Connection;
use crate::connector::Namespace;
use crate::console::{Focus, Phase};
use crate::input::Region;
use crate::store::config::ExplorerSide;
use crate::table::TableFocus;
use crate::ui::browser::draw_browser;
use crate::ui::compare::{draw_compare, draw_compare_form};
use crate::ui::console::BarData;
use crate::ui::diagram::{DiagramBar, draw_diagram};
use crate::ui::export::draw_export;
use crate::ui::form::draw_form;
use crate::ui::help::draw_help;
use crate::ui::history::draw_history;
use crate::ui::import::draw_import;
use crate::ui::kit::feedback::hint_line;
use crate::ui::kit::panel::{draw_panel_title, draw_rule};
use crate::ui::name::draw_name;
use crate::ui::navigator::draw_navigator;
use crate::ui::palette::draw_palette;
use crate::ui::settings::draw_settings;
use crate::ui::statusline::{StatusData, StatusPanel, draw_statusline, run_time};
use crate::ui::welcome::draw_welcome;
use crate::ui::which_key::draw_which_key;
use crate::ui::{
    ConflictRowView, ConsoleLayout, TableBar, draw_conflicts, draw_console, draw_dialog,
    draw_main_tabs, draw_overlay, draw_picker, draw_small_terminal, draw_table, text,
};

impl App {
    pub fn draw(&mut self, frame: &mut Frame) {
        self.choose_theme();
        let area = frame.area();
        self.layout.agent_bar = Default::default();
        self.layout.agent_card = Default::default();
        self.layout.agent_notice = None;
        frame.render_widget(Block::new().style(self.theme.base), area);
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            draw_small_terminal(frame, area, (MIN_WIDTH, MIN_HEIGHT), &self.theme);
            return;
        }
        let tall = area.height >= TALL_HEIGHT;
        let agent_bar = self.agent_bar();
        let [tabs, rule, body, notice, status, hints] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(u16::from(tall)),
            Constraint::Min(0),
            Constraint::Length(u16::from(agent_bar.is_some())),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        self.layout.body = body;
        let views = self.tab_views();
        (self.layout.main_tabs, self.layout.quit) =
            draw_main_tabs(frame, tabs, views, self.active_tab(), &self.theme);
        if tall {
            draw_rule(frame, rule, &self.theme);
        }
        let main = self.draw_side(frame, body);
        match self.tabs.screen {
            Screen::Compare(_) => self.draw_active_compare(frame, main, tall),
            Screen::Diagram(_) => self.draw_active_diagram(frame, main, tall),
            Screen::Table(_) => self.draw_active_table(frame, main, tall),
            Screen::Console if self.home() => self.draw_home(frame, main),
            Screen::Console => self.draw_active_console(frame, main, tall),
        }
        if let Some(bar) = agent_bar {
            self.layout.agent_bar = crate::ui::agent::draw_bar(frame, notice, &bar, &self.theme);
        }
        if let Some((notice, card)) = self.agent_card() {
            self.layout.agent_card = crate::ui::agent::draw_card(frame, main, &card, &self.theme);
            self.layout.agent_notice = Some(notice);
        }
        self.draw_status(frame, status);
        self.draw_hint(frame, hints);
        self.draw_layers(
            frame,
            Rect {
                y: 0,
                height: notice.y,
                ..area
            },
        );
    }

    /// Draws the navigator and its divider. It returns the area for the console.
    pub(super) fn draw_side(&mut self, frame: &mut Frame, body: Rect) -> Rect {
        if self.navigator.collapsed {
            self.layout.navigator_title = Rect::default();
            self.layout.navigator_filter = Rect::default();
            self.layout.navigator = Rect::default();
            self.layout.divider = None;
            return body;
        }
        let width = self
            .navigator
            .width
            .min(body.width.saturating_sub(super::mouse::MIN_MAIN_WIDTH));
        let [side, divider, main] = match self.explorer_side {
            ExplorerSide::Left => {
                let [side, divider, main] = Layout::horizontal([
                    Constraint::Length(width),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .areas(body);
                [side, divider, main]
            }
            ExplorerSide::Right => {
                let [main, divider, side] = Layout::horizontal([
                    Constraint::Min(0),
                    Constraint::Length(1),
                    Constraint::Length(width),
                ])
                .areas(body);
                [side, divider, main]
            }
        };
        let [title, tree] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(side);
        let focused = self.navigator_focused;
        let connected: Vec<bool> = self
            .connections
            .iter()
            .map(|connection| {
                self.manager
                    .as_ref()
                    .is_some_and(|manager| manager.is_connected(connection.id))
            })
            .collect();
        frame.buffer_mut().set_style(side, self.theme.panel);
        draw_panel_title(frame, title, text::TITLE_EXPLORER, focused, &self.theme);
        let (filter, rows) = draw_navigator(
            frame,
            tree,
            &mut self.navigator,
            &self.connections,
            &connected,
            focused,
            &self.theme,
        );
        let line = vec![Line::raw("│"); usize::from(divider.height)];
        frame.render_widget(Paragraph::new(line).style(self.theme.divider), divider);
        self.layout.navigator_title = title;
        self.layout.navigator_filter = filter;
        self.layout.navigator = rows;
        self.layout.divider = Some(divider.x);
        main
    }

    pub(super) fn draw_active_console(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let connection = self.active_connection();
        let name = connection
            .map_or(text::NO_DATABASE_LABEL, |connection| &connection.name)
            .to_owned();
        let namespace = connection.map_or(Namespace::None, |item| item.capabilities.namespace);
        let transactions = connection.is_none_or(|item| item.capabilities.transactions);
        let default_schema = connection.and_then(|item| item.default_schema.clone());
        let parameters_shortcut = self
            .keymap
            .keys(crate::input::Action::FocusParameters)
            .join(", ");
        let bar = BarData {
            parameters_shortcut: &parameters_shortcut,
            connection: &name,
            namespace,
            default_schema: default_schema.as_deref(),
            transactions,
            title: tall,
            welcome: self.welcome(),
            show_system_commands: self.show_system_commands,
            agent: self.agent_tab_without_sql(self.tabs.consoles[self.tabs.active].id),
            agent_sql: self.agent.owns(self.tabs.consoles[self.tabs.active].id),
            agent_access: self
                .agent
                .tab_access(self.tabs.consoles[self.tabs.active].id),
            agent_tasks: self
                .agent
                .tab_tasks(self.tabs.consoles[self.tabs.active].id),
        };
        let ratio = self.editor_ratio;
        let focused = !self.navigator_focused;
        let console = &mut self.tabs.consoles[self.tabs.active];
        self.layout.console = draw_console(frame, area, console, &bar, ratio, focused, &self.theme);
    }

    /// Draws the screen without a tab. The console areas stay empty, so the mouse finds
    /// no editor.
    pub(super) fn draw_home(&mut self, frame: &mut Frame, area: Rect) {
        self.layout.console = ConsoleLayout::default();
        draw_welcome(frame, area, &self.home_screen(), &self.theme);
    }

    pub(super) fn draw_active_table(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let Some(index) = self.table_index() else {
            return;
        };
        let connection = self.tabs.tables[index].connection;
        let name = self
            .connection(connection)
            .map_or_else(String::new, |item| item.name.clone());
        let bar = TableBar {
            connection: &name,
            blocked: self.tabs.tables[index].is_blocked(&self.budget),
            title: tall,
        };
        let focused = !self.navigator_focused;
        self.layout.table = draw_table(
            frame,
            area,
            &mut self.tabs.tables[index],
            &bar,
            focused,
            &self.theme,
        );
    }

    pub(super) fn draw_active_compare(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let Some(index) = self.compare_index() else {
            return;
        };
        let focused = !self.navigator_focused;
        self.layout.compare = draw_compare(
            frame,
            area,
            &mut self.tabs.compares[index],
            tall,
            focused,
            &self.theme,
        );
    }

    pub(super) fn draw_active_diagram(&mut self, frame: &mut Frame, area: Rect, tall: bool) {
        let Some(index) = self.diagram_index() else {
            return;
        };
        let connection = self.tabs.diagrams[index].connection;
        let name = self
            .connection(connection)
            .map_or_else(String::new, |item| item.name.clone());
        let bar = DiagramBar {
            connection: &name,
            title: tall,
        };
        let focused = !self.navigator_focused;
        self.layout.diagram = draw_diagram(
            frame,
            area,
            &mut self.tabs.diagrams[index],
            &bar,
            focused,
            &self.theme,
        );
    }

    /// Returns the name of the place that receives keys, for the status line.
    pub(super) fn status_panel(&self) -> StatusPanel {
        if self.navigator_focused {
            return if self.navigator.filter_is_editing() {
                StatusPanel::Filter
            } else {
                StatusPanel::Explorer
            };
        }
        if self.diagram_index().is_some() {
            return StatusPanel::Diagram;
        }
        if self.compare_index().is_some() {
            return StatusPanel::Table;
        }
        if self.home() {
            return StatusPanel::Home;
        }
        if let Some(tab) = self.table() {
            return if tab.focus == TableFocus::Grid {
                StatusPanel::Table
            } else {
                StatusPanel::Filter
            };
        }
        let console = self.console();
        if console.focus == Focus::Results {
            return StatusPanel::Results;
        }
        if console.focus == Focus::Parameters {
            return StatusPanel::Filter;
        }
        if !self.vim() {
            return StatusPanel::Editor;
        }
        match console.editor.state.mode {
            EditorMode::Insert => StatusPanel::Insert,
            EditorMode::Visual => StatusPanel::Visual,
            _ => StatusPanel::Normal,
        }
    }

    /// Writes the effective key of an agent command, or its path in the palette.
    pub(super) fn agent_shortcut(
        &self,
        action: crate::input::Action,
        name: &str,
        entry: &str,
    ) -> Option<String> {
        self.shown_key(action)
            .map(|key| format!("{key}: {name}"))
            .or_else(|| {
                self.shown_key(crate::input::Action::OpenPalette)
                    .map(|key| format!("{key} > {entry}"))
            })
    }

    /// Returns the MCP row while an item waits for the user or an agent task runs.
    pub(super) fn agent_bar(&self) -> Option<crate::ui::agent::Bar> {
        let access = self.agent.pending_access();
        let proposals = self.agent.pending_proposals();
        let tasks = self.agent.active_tasks();
        if access + proposals + tasks == 0 {
            return None;
        }
        let (action, name, entry) = if access != 0 {
            (
                crate::input::Action::AgentSessions,
                "sessions",
                "Agent sessions",
            )
        } else {
            (
                crate::input::Action::AgentReview,
                "review",
                "Review agent changes",
            )
        };
        let shortcut = (access + proposals != 0).then(|| self.agent_shortcut(action, name, entry));
        Some(crate::ui::agent::Bar {
            items: text::agent_bar(access, proposals, tasks),
            shortcut: shortcut.flatten(),
            review: access + proposals != 0,
            stop: tasks != 0,
        })
    }

    pub(super) fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let panel = self.status_panel();
        let data = (self.budget.used(), self.budget.limit());
        let agent = self.agent.status_label();
        let agent = agent.as_deref();
        if let Some(tab) = self.compare() {
            let view = tab.result.view;
            let position =
                (!tab.result.table.rows.is_empty()).then_some((view.row + 1, view.col + 1));
            let target = &tab.sides[1].side;
            let item = self.connection(target.connection);
            let status = StatusData {
                panel,
                connection: item.map_or("", |item| item.name.as_str()),
                production: item.is_some_and(Connection::is_production),
                schema: Some(&target.schema),
                selected_characters: 0,
                session: None,
                transaction_age: None,
                phase: tab.phase,
                live: false,
                run_time: run_time(tab.phase, &tab.run, self.time.now),
                pool: self
                    .manager
                    .as_ref()
                    .map(|manager| manager.usage(target.connection)),
                data,
                position,
                agent,
                auto: None,
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        if let Some(tab) = self.diagram() {
            let item = self.connection(tab.connection);
            let status = StatusData {
                panel,
                connection: item.map_or("", |item| item.name.as_str()),
                production: item.is_some_and(Connection::is_production),
                schema: Some(tab.focus.schema()),
                selected_characters: 0,
                session: None,
                transaction_age: None,
                phase: tab.phase,
                live: false,
                run_time: run_time(tab.phase, &tab.run, self.time.now),
                pool: self
                    .manager
                    .as_ref()
                    .map(|manager| manager.usage(tab.connection)),
                data,
                position: None,
                agent,
                auto: None,
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        if let Some(tab) = self.table() {
            let view = tab.result.view;
            let position =
                (!tab.result.table.rows.is_empty()).then_some((view.row + 1, view.col + 1));
            let item = self.connection(tab.connection);
            let status = StatusData {
                panel,
                connection: item.map_or("", |item| item.name.as_str()),
                production: item.is_some_and(Connection::is_production),
                schema: Some(&tab.relation.schema),
                selected_characters: 0,
                session: None,
                transaction_age: None,
                phase: tab.phase,
                live: false,
                run_time: run_time(tab.phase, &tab.run, self.time.now),
                pool: self
                    .manager
                    .as_ref()
                    .map(|manager| manager.usage(tab.connection)),
                data,
                position,
                agent,
                auto: tab.auto_refresh.map(|auto| auto.every.as_secs()),
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        if self.home() {
            let status = StatusData {
                panel,
                connection: "",
                selected_characters: 0,
                production: false,
                schema: None,
                session: None,
                transaction_age: None,
                phase: Phase::Idle,
                live: false,
                run_time: None,
                pool: None,
                data,
                position: None,
                agent,
                auto: None,
            };
            draw_statusline(frame, area, &status, &self.theme);
            return;
        }
        let console = self.console();
        let connection = self.active_connection();
        let namespace = connection.map_or(Namespace::None, |item| item.capabilities.namespace);
        let default_schema = connection.and_then(|item| item.default_schema.as_deref());
        let result = console
            .results
            .get(console.active)
            .filter(|_| console.focus == Focus::Results);
        let position = match result {
            Some(result) => (!result.table.rows.is_empty())
                .then_some((result.view.row + 1, result.view.col + 1)),
            None => {
                let cursor = console.editor.cursor();
                Some((cursor.row + 1, cursor.col + 1))
            }
        };
        let status = StatusData {
            panel,
            connection: connection.map_or(text::NO_DATABASE_LABEL, |item| item.name.as_str()),
            selected_characters: if console.focus == Focus::Editor && !self.navigator_focused {
                console.editor.selected_character_count()
            } else {
                0
            },
            production: connection.is_some_and(Connection::is_production),
            schema: (namespace != Namespace::None)
                .then(|| console.schema.as_deref().or(default_schema))
                .flatten(),
            session: Some((console.manual, console.transaction)),
            transaction_age: console.transaction_age(self.time.now),
            phase: console.phase,
            live: console.is_live(),
            run_time: run_time(console.phase, &console.run, self.time.now),
            pool: console
                .connection
                .zip(self.manager.as_ref())
                .map(|(id, manager)| manager.usage(id)),
            data,
            position,
            agent,
            auto: console.auto_refresh.map(|auto| auto.every.as_secs()),
        };
        draw_statusline(frame, area, &status, &self.theme);
    }

    pub(super) fn draw_hint(&self, frame: &mut Frame, area: Rect) {
        let message = self
            .status
            .clone()
            .or_else(|| self.export_progress())
            .or_else(|| self.import_progress())
            .or_else(|| self.auth_wait().map(str::to_owned));
        // The row starts one column after the edge, as the status line does.
        let area = Rect {
            x: area.x + 1,
            width: area.width.saturating_sub(1),
            ..area
        };
        let line = match message {
            Some(status) => Line::styled(status, self.theme.status),
            None => {
                let items = self.fitted_hints(usize::from(area.width));
                hint_line(&items, &self.theme)
            }
        };
        frame.render_widget(Paragraph::new(line), area);
    }

    /// Draws the windows above the panels. `bounds` holds the rows above the status line.
    pub(super) fn draw_layers(&mut self, frame: &mut Frame, bounds: Rect) {
        self.draw_ai(frame);
        if self.input.command_page.is_some_and(|region| {
            region != Region::Navigator && self.context_region() != Some(region)
        }) {
            self.input.command_page = None;
        }
        if let Some(region) = self.input.command_page {
            let (title, menu) = if region == Region::Navigator {
                (self.explorer_menu_title(), self.explorer_menu())
            } else {
                (self.context_menu_title(), self.context_menu())
            };
            draw_which_key(frame, bounds, &title, &menu, &self.theme);
        } else if self.pending_prefix() {
            let title = text::TITLE_PREFIX;
            draw_which_key(frame, bounds, title, &self.prefix_menu(), &self.theme);
        }
        // The windows are drawn from the bottom to the top. The AI prompts are under the
        // question dialog.
        for modal in Modal::ALL.into_iter().rev() {
            if modal == Modal::Dialog {
                self.draw_ai_prompts(frame);
            }
            self.draw_modal(frame, modal);
        }
    }

    /// Draws a window over the screen when it is open.
    pub(super) fn draw_modal(&mut self, frame: &mut Frame, modal: Modal) {
        match modal {
            Modal::Form => {
                if let Some(form) = &self.modals.form {
                    self.layout.form = draw_form(frame, form, &self.theme);
                }
            }
            Modal::Picker => {
                if let Some(picker) = &self.modals.picker {
                    let items: Vec<&str> = picker.items.iter().map(String::as_str).collect();
                    self.layout.picker = draw_picker(
                        frame,
                        &picker.title,
                        &items,
                        picker.cursor,
                        picker.loading,
                        &self.theme,
                    );
                }
            }
            Modal::Conflicts => {
                if let Some(conflicts) = &self.modals.conflicts {
                    let rows: Vec<ConflictRowView> = conflicts
                        .rows
                        .iter()
                        .map(|row| ConflictRowView {
                            name: &row.name,
                            removed: row.removed,
                            choice: row.choice.map(|choice| row.options()[choice]),
                        })
                        .collect();
                    self.layout.conflicts =
                        draw_conflicts(frame, &rows, conflicts.cursor, conflicts.note, &self.theme);
                }
            }
            Modal::Overlay => {
                if let Some(overlay) = &mut self.modals.overlay {
                    let Overlay {
                        title,
                        contents,
                        footer,
                        scroll,
                        ..
                    } = overlay;
                    *scroll = draw_overlay(frame, title, contents, footer, *scroll, &self.theme);
                    self.layout.overlay = crate::ui::console::overlay_text_area(
                        frame.area(),
                        contents.search().is_some(),
                    );
                }
            }
            Modal::Help => {
                if let Some(help) = &mut self.modals.help {
                    draw_help(frame, help, &self.theme);
                }
            }
            Modal::Palette => {
                if let Some(palette) = &self.modals.palette {
                    self.layout.palette = draw_palette(frame, palette, &self.theme);
                }
            }
            Modal::History => {
                if let Some(list) = &self.modals.history {
                    self.layout.history = draw_history(frame, list, &self.theme);
                }
            }
            Modal::Settings => {
                if let Some(form) = &self.modals.settings_form {
                    self.layout.settings = draw_settings(frame, form, &self.theme);
                }
            }
            Modal::Export => {
                if let Some(form) = &self.modals.export_form {
                    self.layout.export = draw_export(frame, form, &self.theme);
                }
            }
            Modal::Edit => {
                if let Some(form) = &mut self.modals.edit_form {
                    self.layout.edit = crate::ui::data_edit::draw_edit(frame, form, &self.theme);
                }
            }
            Modal::Import => {
                if let Some(form) = &self.modals.import_form {
                    self.layout.import = draw_import(frame, form, &self.theme);
                }
            }
            Modal::Compare => {
                if let Some(form) = &self.modals.compare_form {
                    self.layout.compare_form = draw_compare_form(frame, form, &self.theme);
                }
            }
            Modal::Browser => {
                if let Some(browser) = &self.modals.browser {
                    self.layout.browser = draw_browser(frame, browser, &self.theme);
                }
            }
            Modal::Rename => {
                if let Some(prompt) = &self.modals.rename {
                    draw_name(frame, prompt, &self.theme);
                }
            }
            Modal::Password => {
                if let Some(prompt) = &self.auth.prompt {
                    self.layout.password =
                        crate::ui::auth::draw_password(frame, prompt, &self.theme);
                }
            }
            Modal::Dialog => {
                if let Some(dialog) = self.modals.dialogs.front() {
                    let labels: Vec<&str> =
                        dialog.options.iter().map(|(label, _)| *label).collect();
                    self.layout.dialog = draw_dialog(
                        frame,
                        &dialog.title,
                        &dialog.body,
                        &labels,
                        dialog.focus,
                        &self.theme,
                    );
                }
            }
        }
    }
}
