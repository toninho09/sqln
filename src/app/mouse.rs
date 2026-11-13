//! The mouse: clicks, the wheel, and the drag of dividers and columns.

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::{App, MAX_EDITOR_RATIO, MIN_EDITOR_RATIO};
use crate::console::Focus;
use crate::export::form::Field;
use crate::filter_list::ListRequest;
use crate::form::FormRequest;
use crate::input::Action;
use crate::navigator::{MIN_WIDTH, Navigator, RowKind};
use crate::result::ResultTab;
use crate::settings::{Field as SettingsField, SettingsRequest};
use crate::store::config::ExplorerSide;
use crate::table::TableFocus;
use crate::ui::console::Control;
use crate::ui::navigator::TREE_PAD;
use crate::ui::{
    MainTab, TableControl, column_edge_at, grid_cell_at, grid_row_number_at, header_column_at,
    section_at, tab_at,
};

/// The console keeps this width when the user drags the divider of the navigator.
pub(super) const MIN_MAIN_WIDTH: u16 = 40;
const WHEEL_ROWS: isize = 3;
/// One gesture of a touchpad sends many events, so one event moves one column.
const WHEEL_COLUMNS: isize = 1;
/// Two clicks on the same cell in this time open a table.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

#[derive(Clone, Copy)]
pub(super) enum Drag {
    /// The edge of a column in the grid header.
    Column {
        column: usize,
        start: u16,
        width: usize,
    },
    Navigator,
    Editor,
    /// The picture of a diagram, with the last place of the pointer.
    Diagram {
        column: u16,
        row: u16,
    },
}

impl App {
    pub(super) fn on_overlay_mouse(&mut self, mouse: MouseEvent) {
        let area = self.layout.overlay;
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        let position = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp => overlay.scroll = overlay.scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => overlay.scroll = overlay.scroll.saturating_add(3),
            MouseEventKind::Down(MouseButton::Left) if area.contains(position) => {
                overlay.dragging = true;
                overlay.contents.select_at(
                    overlay.scroll + usize::from(mouse.row - area.y),
                    usize::from(mouse.column - area.x),
                    mouse
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::SHIFT),
                );
            }
            MouseEventKind::Drag(MouseButton::Left) if overlay.dragging && !area.is_empty() => {
                if mouse.row < area.y {
                    overlay.scroll = overlay.scroll.saturating_sub(1);
                } else if mouse.row >= area.bottom() {
                    overlay.scroll = overlay.scroll.saturating_add(1);
                }
                overlay.scroll = overlay.contents.last_row_at(overlay.scroll);
                let row = mouse.row.clamp(area.y, area.bottom() - 1) - area.y;
                let column = mouse.column.clamp(area.x, area.right()) - area.x;
                overlay.contents.select_at(
                    overlay.scroll + usize::from(row),
                    usize::from(column),
                    true,
                );
            }
            MouseEventKind::Up(MouseButton::Left) => overlay.dragging = false,
            _ => {}
        }
    }

    pub(super) fn on_mouse(&mut self, mouse: MouseEvent) {
        if self.form.is_some() {
            self.on_form_mouse(mouse);
            return;
        }
        if self.editor_has_focus() && self.console().editor.dragging_scrollbar() {
            self.console_mut().editor.on_mouse(mouse);
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        if !self.layout.agent_card.area.contains(position) && self.on_review_mouse(mouse) {
            return;
        }
        match mouse.kind {
            // The access card is above the panels, so it takes their clicks.
            MouseEventKind::Down(MouseButton::Left)
                if self.layout.agent_card.area.contains(position) =>
            {
                self.on_agent_card(position);
            }
            MouseEventKind::Down(MouseButton::Left) => self.on_press(mouse, position),
            MouseEventKind::Drag(MouseButton::Left) if self.drag.is_some() => self.on_drag(mouse),
            MouseEventKind::Up(_) if self.drag.is_some() => self.drag = None,
            MouseEventKind::Drag(MouseButton::Left) if self.grid_has_focus() => {
                self.point_at_cell(mouse, true);
            }
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => self.on_wheel(mouse, position),
            _ if self.layout.console.editor.contains(position) && self.editor_has_focus() => {
                self.console_mut().editor.on_mouse(mouse);
            }
            _ => {}
        }
    }

    /// Returns the result grid that the mouse acts on, with its area on the screen.
    fn active_grid(&mut self) -> Option<(&mut ResultTab, Rect)> {
        if self.diagram_active.is_some() {
            return None;
        }
        if let Some(index) = self.compare_active {
            let tab = &mut self.compares[index];
            return tab.done.then_some((&mut tab.result, self.layout.compare));
        }
        if let Some(index) = self.table_active {
            return Some((&mut self.tables[index].result, self.layout.table.grid));
        }
        let area = self.layout.console.results;
        let console = &mut self.consoles[self.active];
        let active = console.active;
        console.results.get_mut(active).map(|result| (result, area))
    }

    fn on_press(&mut self, mouse: MouseEvent, position: Position) {
        let layout = &self.layout;
        if layout.divider == Some(mouse.column) && layout.body.contains(position) {
            self.drag = Some(Drag::Navigator);
        } else if layout.quit.contains(position) {
            self.request_exit();
        } else if layout.agent_bar.review.contains(position) {
            self.pending_key = None;
            self.command_page = None;
            self.review_agent_next();
        } else if layout.agent_bar.stop.contains(position) {
            if self.command_available(Action::AgentStop) {
                self.pending_key = None;
                self.command_page = None;
                self.perform(Action::AgentStop, false);
            }
        } else if let Some(tab) = main_tab_at(&layout.main_tabs, position) {
            if tab.close.is_some_and(|close| mouse.column >= close) {
                self.close_tab(tab.index);
            } else {
                self.select_tab(tab.index);
            }
        } else if layout.navigator_filter.contains(position) {
            self.focus_navigator();
            self.navigator.open_filter();
        } else if layout.navigator.contains(position) {
            self.on_navigator_press(mouse);
        } else if layout.navigator_title.contains(position) {
            self.focus_navigator();
        } else if self.diagram_active.is_some() {
            self.on_diagram_press(mouse, position);
        } else if self.compare_active.is_some() {
            self.on_compare_press(mouse, position);
        } else if self.table_active.is_some() {
            self.on_table_press(mouse, position);
        } else {
            self.on_console_press(mouse, position);
        }
    }

    fn on_console_press(&mut self, mouse: MouseEvent, position: Position) {
        let layout = &self.layout;
        let in_tabs = layout.console.tabs.contains(position);
        let in_results = layout.console.results.contains(position);
        let in_editor = layout.console.editor.contains(position);
        let in_controls = layout.console.controls.contains(position);
        if self.layout.console.parameters.header.contains(position) {
            self.perform(Action::ToggleParameters, false);
        } else if let Some((index, column, rect)) = self
            .layout
            .console
            .parameters
            .rows
            .iter()
            .find_map(|(index, areas)| {
                areas
                    .iter()
                    .enumerate()
                    .find(|(_, area)| area.contains(position))
                    .map(|(column, rect)| (*index, column, *rect))
            })
        {
            let was_editing = !self.navigator_focused
                && self.console().focus == Focus::Parameters
                && self.console().parameters.selected == index
                && self.console().parameters.column == 1;
            self.navigator_focused = false;
            let console = self.console_mut();
            console.focus = Focus::Parameters;
            console.editor.close_suggestions();
            console.parameters.selected = index;
            console.parameters.column = column;
            if column == 1 {
                let field = &mut console.parameters.fields[index];
                let visible = rect.width as usize;
                let text = field.input.text();
                let start = if was_editing {
                    crate::ui::parameters::input_start(&field.input, visible)
                } else {
                    0
                };
                let mut width = 0;
                let mut byte = start;
                for (offset, c) in text[start..].char_indices() {
                    if width >= usize::from(mouse.column.saturating_sub(rect.x)) {
                        break;
                    }
                    width += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                    byte = start + offset + c.len_utf8();
                }
                field.input.set_byte_cursor(byte);
            } else {
                console.parameters.on_key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Enter,
                    crossterm::event::KeyModifiers::NONE,
                ));
            }
        } else if in_controls {
            self.on_control_press(position);
        } else if in_tabs {
            match tab_at(self.console(), self.layout.console.tabs, mouse.column) {
                Some(tab) => self.console_mut().active = tab,
                None => self.drag = Some(Drag::Editor),
            }
        } else if in_results {
            self.navigator_focused = false;
            self.console_mut().focus = Focus::Results;
            if !self.start_column_drag(mouse) {
                self.point_at_cell(mouse, false);
            }
        } else if in_editor {
            self.navigator_focused = false;
            self.console_mut().focus = Focus::Editor;
            self.console_mut().editor.on_mouse(mouse);
        }
    }

    /// Returns true when this click and the one before it hit the same cell.
    fn is_double_click(&mut self, mouse: MouseEvent) -> bool {
        let now = Instant::now();
        let double = self.last_click.is_some_and(|(at, column, row)| {
            column == mouse.column && row == mouse.row && now.duration_since(at) <= DOUBLE_CLICK
        });
        self.last_click = if double {
            None
        } else {
            Some((now, mouse.column, mouse.row))
        };
        double
    }

    fn on_table_press(&mut self, mouse: MouseEvent, position: Position) {
        let layout = &self.layout.table;
        let area = layout.grid;
        if layout.controls.contains(position) {
            self.on_table_control_press(mouse.column);
        } else if layout.sections.contains(position) {
            if let Some(section) = section_at(layout.sections, mouse.column) {
                self.focus_table(TableFocus::Grid);
                self.show_section(section);
            }
        } else if layout.summary.contains(position) {
            self.on_pager_press(mouse.column);
        } else if layout.filter.contains(position) {
            self.focus_table(TableFocus::Filter);
        } else if layout.order.contains(position) {
            self.focus_table(TableFocus::Order);
        } else if area.contains(position) {
            self.focus_table(TableFocus::Grid);
            if self.start_column_drag(mouse) {
                return;
            }
            let header = self
                .table()
                .and_then(|tab| header_column_at(&tab.result, area, mouse.column, mouse.row));
            match header {
                Some(column) => self.sort_table(column),
                None => self.point_at_cell(mouse, false),
            }
        }
    }

    /// Puts the focus on the grid of the compare and the cursor on the cell under the
    /// pointer. A drag of a column edge changes its width.
    fn on_compare_press(&mut self, mouse: MouseEvent, position: Position) {
        if !self.layout.compare.contains(position) {
            return;
        }
        self.navigator_focused = false;
        if !self.start_column_drag(mouse) {
            self.point_at_cell(mouse, false);
        }
    }

    fn on_table_control_press(&mut self, column: u16) {
        let buttons = &self.layout.table.buttons;
        let control = buttons
            .iter()
            .find(|(_, rect)| column >= rect.x && column < rect.right());
        match control.map(|(control, _)| *control) {
            Some(TableControl::Apply) => self.apply_table(),
            Some(TableControl::Clear) => self.clear_table(),
            Some(TableControl::ApplyEdits) => {
                self.perform_on_table(Action::GridSaveEdits);
            }
            Some(TableControl::ReviewEdits) => {
                self.perform_on_table(Action::GridReviewEdits);
            }
            Some(TableControl::DiscardEdits) => {
                self.perform_on_table(Action::GridDiscardEdits);
            }
            Some(TableControl::LoadMore) => self.load_more(),
            Some(TableControl::Stop) => self.stop_table(),
            _ => {}
        }
    }

    fn on_pager_press(&mut self, column: u16) {
        let buttons = &self.layout.table.pager;
        let control = buttons
            .iter()
            .find(|(_, rect)| column >= rect.x && column < rect.right());
        let Some(control) = control.map(|(control, _)| *control) else {
            return;
        };
        self.focus_table(TableFocus::Grid);
        match control {
            TableControl::PreviousPage => self.previous_table_page(),
            TableControl::NextPage => self.next_table_page(),
            _ => {}
        }
    }

    fn on_navigator_press(&mut self, mouse: MouseEvent) {
        self.navigator_focused = true;
        let area = self.layout.navigator;
        let index = self.navigator.top + usize::from(mouse.row - area.y);
        let rows = self.navigator.rows(&self.connections);
        let Some(row) = rows.get(index).copied() else {
            return;
        };
        self.navigator.select_index(index, &self.connections);
        // The mark of a branch stands after the indent of its row.
        let mark = usize::from(area.x + TREE_PAD) + row.depth() * 2;
        if usize::from(mouse.column) == mark && Navigator::is_branch(&row) {
            self.toggle(&row);
        } else if self.is_double_click(mouse) {
            match row.kind {
                RowKind::Table | RowKind::View => self.open_table(&row),
                RowKind::Console => self.open_tree_console(&row),
                RowKind::Object(_) => self.open_row_definition(&row),
                _ => {}
            }
        }
    }

    fn on_control_press(&mut self, position: Position) {
        let control = self
            .layout
            .console
            .control_buttons
            .iter()
            .find(|(_, rect)| rect.contains(position));
        let Some((control, _)) = control else {
            return;
        };
        let action = match *control {
            Control::Run => Action::Run,
            Control::Script => Action::RunScript,
            Control::Stop => Action::Stop,
            Control::Mode => Action::ToggleMode,
            Control::Commit => Action::Commit,
            Control::Rollback => Action::Rollback,
            Control::Schema => Action::ChooseSchema,
            Control::NewConsole => Action::AgentSqlToConsole,
            Control::Clear => Action::AgentSqlClear,
            Control::Access => Action::AgentSession,
            Control::Tasks => Action::AgentTasks,
        };
        self.perform(action, false);
    }

    fn on_wheel(&mut self, mouse: MouseEvent, position: Position) {
        if self.diagram_active.is_some() && self.on_diagram_wheel(mouse, position) {
            return;
        }
        let (rows, columns) = match mouse.kind {
            MouseEventKind::ScrollUp => (-WHEEL_ROWS, 0),
            MouseEventKind::ScrollDown => (WHEEL_ROWS, 0),
            MouseEventKind::ScrollLeft => (0, -WHEEL_COLUMNS),
            MouseEventKind::ScrollRight => (0, WHEEL_COLUMNS),
            _ => return,
        };
        // Only the grid and the editor move to the side.
        let vertical = columns == 0;
        if vertical
            && self.console_on_screen()
            && self.layout.console.parameters.area.contains(position)
        {
            let panel = &mut self.console_mut().parameters;
            panel.selected = panel
                .selected
                .saturating_add_signed(rows)
                .min(panel.fields.len().saturating_sub(1));
        } else if vertical && self.layout.navigator.contains(position) {
            self.navigator.move_by(rows, &self.connections);
        } else if vertical
            && self.console_on_screen()
            && self.console().showing_messages()
            && self.layout.console.results.contains(position)
        {
            self.console_mut().scroll_messages(rows);
        } else if let Some((result, area)) = self.active_grid()
            && area.contains(position)
        {
            result.view.move_by(rows, columns, &result.table, false);
        } else if self.layout.console.editor.contains(position) && self.editor_has_focus() {
            self.console_mut().editor.on_mouse(mouse);
        }
    }

    fn on_drag(&mut self, mouse: MouseEvent) {
        match self.drag {
            Some(Drag::Column { .. }) => self.drag_column(mouse),
            Some(Drag::Navigator) => {
                let body = self.layout.body;
                let limit = body.width.saturating_sub(MIN_MAIN_WIDTH);
                let width = match self.explorer_side {
                    ExplorerSide::Left => mouse.column.saturating_sub(body.x),
                    // The divider is the column before the explorer.
                    ExplorerSide::Right => {
                        body.right().saturating_sub(mouse.column.saturating_add(1))
                    }
                };
                self.navigator.width = width.clamp(MIN_WIDTH.min(limit), limit);
            }
            Some(Drag::Diagram { .. }) => self.drag_diagram(mouse),
            Some(Drag::Editor) => {
                let area = self.layout.console.split;
                if area.height > 0 {
                    let rows = mouse.row.saturating_sub(area.y);
                    let ratio = u32::from(rows) * 100 / u32::from(area.height);
                    let ratio = u16::try_from(ratio).unwrap_or(MAX_EDITOR_RATIO);
                    self.editor_ratio = ratio.clamp(MIN_EDITOR_RATIO, MAX_EDITOR_RATIO);
                }
            }
            None => {}
        }
    }

    fn start_column_drag(&mut self, mouse: MouseEvent) -> bool {
        let Some((result, area)) = self.active_grid() else {
            return false;
        };
        let edge =
            column_edge_at(result, area, mouse.column, mouse.row).map(|column| Drag::Column {
                column,
                start: mouse.column,
                width: result.table.width(column),
            });
        self.drag = edge;
        self.drag.is_some()
    }

    fn drag_column(&mut self, mouse: MouseEvent) {
        let Some(Drag::Column {
            column,
            start,
            width,
        }) = self.drag
        else {
            return;
        };
        let Some((result, _)) = self.active_grid() else {
            return;
        };
        let change = isize::from(mouse.column as i16 - start as i16);
        result
            .table
            .set_width(column, width.saturating_add_signed(change));
    }

    fn point_at_cell(&mut self, mouse: MouseEvent, extend: bool) {
        let Some((result, area)) = self.active_grid() else {
            return;
        };
        if !extend && let Some(row) = grid_row_number_at(result, area, mouse.column, mouse.row) {
            result.view.select_row(row, &result.table);
        } else if let Some((row, col)) = grid_cell_at(result, area, mouse.column, mouse.row) {
            result.view.move_to(row, col, &result.table, extend);
        }
    }

    pub(super) fn on_dialog_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        let (options, row) = &self.layout.dialog;
        let option = options
            .iter()
            .find(|(start, end, _)| {
                mouse.row == *row && mouse.column >= *start && mouse.column < *end
            })
            .map(|(_, _, index)| *index);
        let Some(index) = option else { return };
        if let Some(dialog) = self.dialogs.pop_front() {
            self.on_dialog_action(dialog.options[index].1.clone());
        }
    }

    pub(super) fn on_picker_mouse(&mut self, mouse: MouseEvent) {
        let (area, first) = self.layout.picker;
        let inside = area.contains(Position::new(mouse.column, mouse.row));
        let Some(picker) = &mut self.picker else {
            return;
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let index = first + usize::from(mouse.row - area.y);
                if index < picker.items.len() {
                    picker.cursor = index;
                    self.accept_picker();
                }
            }
            MouseEventKind::ScrollUp => picker.cursor = picker.cursor.saturating_sub(1),
            MouseEventKind::ScrollDown => {
                picker.cursor = (picker.cursor + 1).min(picker.items.len().saturating_sub(1));
            }
            _ => {}
        }
    }

    /// A click moves the cursor, and a second click opens the row. With one click, the
    /// second click of a habit would act on a row of the next folder.
    pub(super) fn on_browser_mouse(&mut self, mouse: MouseEvent) {
        let (area, first) = self.layout.browser;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if area.contains(Position::new(mouse.column, mouse.row)) =>
            {
                let index = first + usize::from(mouse.row - area.y);
                let double = self.is_double_click(mouse);
                let Some(browser) = &mut self.browser else {
                    return;
                };
                browser.point_at(index);
                if double {
                    let request = browser.open(index);
                    self.on_browser_request(request);
                }
            }
            MouseEventKind::ScrollUp => self.scroll_browser(-WHEEL_ROWS),
            MouseEventKind::ScrollDown => self.scroll_browser(WHEEL_ROWS),
            _ => {}
        }
    }

    fn scroll_browser(&mut self, rows: isize) {
        if let Some(browser) = &mut self.browser {
            browser.move_by(rows);
        }
    }

    pub(super) fn on_palette_mouse(&mut self, mouse: MouseEvent) {
        let (area, first) = self.layout.palette;
        let inside = area.contains(Position::new(mouse.column, mouse.row));
        let Some(palette) = &mut self.palette else {
            return;
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let request = palette.choose(first + usize::from(mouse.row - area.y));
                self.on_palette_request(request);
            }
            MouseEventKind::ScrollUp => palette.move_by(-1),
            MouseEventKind::ScrollDown => palette.move_by(1),
            _ => {}
        }
    }

    pub(super) fn on_history_mouse(&mut self, mouse: MouseEvent) {
        let (area, first) = self.layout.history;
        let inside = area.contains(Position::new(mouse.column, mouse.row));
        let Some(list) = &mut self.history else {
            return;
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let index = first + usize::from(mouse.row - area.y);
                if index < list.shown().len() {
                    self.on_history_request(ListRequest::Choose(index));
                }
            }
            MouseEventKind::ScrollUp => list.move_by(-1),
            MouseEventKind::ScrollDown => list.move_by(1),
            _ => {}
        }
    }

    fn on_form_mouse(&mut self, mouse: MouseEvent) {
        if self
            .form
            .as_ref()
            .is_some_and(|form| form.schema_list.is_some())
        {
            self.on_schema_list_mouse(mouse);
            return;
        }
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        let layout = &self.layout.form;
        let inside = |(_, area): &&(_, Rect)| area.contains(position);
        let section = layout
            .sections
            .iter()
            .find(|(_, area)| area.contains(position));
        if let Some((section, _)) = section {
            let section = *section;
            if let Some(form) = &mut self.form {
                form.select_section(section);
            }
            return;
        }
        let button = layout.browse.iter().find(inside);
        let Some((chosen, _)) = button.or(layout.fields.iter().find(inside)) else {
            return;
        };
        let (chosen, browse) = (*chosen, button.is_some());
        let Some(form) = &mut self.form else { return };
        form.focus_on(chosen);
        if browse {
            self.open_form_window();
            return;
        }
        let request = form.press();
        match request {
            FormRequest::Test => self.test_form(),
            FormRequest::Save => self.save_form(),
            FormRequest::Disconnect => self.disconnect_form(),
            FormRequest::Cancel => self.form = None,
            FormRequest::Browse => self.open_form_window(),
            FormRequest::None => {}
        }
    }

    /// A click adds or removes the mark of a row. The wheel moves the cursor.
    fn on_schema_list_mouse(&mut self, mouse: MouseEvent) {
        let (area, first) = self.layout.form.list;
        let inside = area.contains(Position::new(mouse.column, mouse.row));
        let Some(open) = self
            .form
            .as_mut()
            .and_then(|form| form.schema_list.as_mut())
        else {
            return;
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside && !open.loading => {
                open.list.toggle(first + usize::from(mouse.row - area.y));
            }
            MouseEventKind::ScrollUp => open.list.move_by(-1),
            MouseEventKind::ScrollDown => open.list.move_by(1),
            _ => {}
        }
    }

    pub(super) fn on_settings_mouse(&mut self, mouse: MouseEvent) {
        let position = Position::new(mouse.column, mouse.row);
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let settings = &self.layout.settings;
            let inside_commands = settings.fields.iter().any(|(field, area)| {
                *field == SettingsField::ShortcutCommands && area.contains(position)
            });
            let inside_colors = settings.fields.iter().any(|(field, area)| {
                *field == SettingsField::ThemeColors && area.contains(position)
            });
            if let Some(form) = &mut self.settings_form {
                let delta = if mouse.kind == MouseEventKind::ScrollUp {
                    -WHEEL_ROWS
                } else {
                    WHEEL_ROWS
                };
                if inside_commands {
                    form.move_shortcut_command(delta);
                } else if inside_colors {
                    form.scroll_theme_color(delta);
                }
            }
            return;
        }
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        if let Some((action, _)) = self
            .layout
            .settings
            .command_rows
            .iter()
            .find(|(_, area)| area.contains(position))
        {
            if let Some(form) = &mut self.settings_form {
                form.edit_shortcut(*action);
            }
            return;
        }
        if let Some((id, _)) = self
            .layout
            .settings
            .theme_rows
            .iter()
            .find(|(_, area)| area.contains(position))
        {
            if let Some(form) = &mut self.settings_form {
                form.select_theme(id);
            }
            return;
        }
        if let Some((role, _)) = self
            .layout
            .settings
            .color_rows
            .iter()
            .find(|(_, area)| area.contains(position))
        {
            if let Some(form) = &mut self.settings_form {
                form.focus_theme_color(*role);
            }
            return;
        }
        let sections = &self.layout.settings.sections;
        if let Some(section) = sections.iter().position(|area| area.contains(position)) {
            if let Some(form) = &mut self.settings_form {
                form.open_section(section);
            }
            return;
        }
        let field = self
            .layout
            .settings
            .fields
            .iter()
            .find(|(_, area)| area.contains(position));
        let Some((field, _)) = field else { return };
        let field = *field;
        let Some(form) = &mut self.settings_form else {
            return;
        };
        form.focus_on(field);
        let request = form.press();
        match request {
            SettingsRequest::AiTest => self.test_ai_connection(),
            SettingsRequest::Save => self.save_settings(),
            SettingsRequest::Cancel => self.close_settings(),
            SettingsRequest::None => {}
        }
    }
}

impl App {
    pub(super) fn on_export_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        let layout = &self.layout.export;
        let browse = layout.browse.is_some_and(|area| area.contains(position));
        let format = layout
            .formats
            .iter()
            .position(|area| area.contains(position));
        let field = layout
            .fields
            .iter()
            .find(|(_, area)| area.contains(position))
            .map(|(field, _)| *field);
        let Some(form) = &mut self.export_form else {
            return;
        };
        if browse {
            form.focus_on(Field::File);
            self.open_browser();
        } else if let Some(format) = format {
            form.open_format(format);
        } else if let Some(field) = field {
            form.focus_on(field);
            let request = form.press();
            self.on_export_request(request);
        }
    }
}

impl App {
    pub(super) fn on_import_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        let layout = &self.layout.import;
        let browse = layout.browse.filter(|(_, area)| area.contains(position));
        let section = layout
            .sections
            .iter()
            .position(|area| area.contains(position));
        let column = layout
            .columns
            .iter()
            .find(|(_, area)| area.contains(position))
            .map(|(column, _)| *column);
        let field = layout
            .fields
            .iter()
            .find(|(_, area)| area.contains(position))
            .map(|(field, _)| *field);
        let Some(form) = &mut self.import_form else {
            return;
        };
        if let Some((path, _)) = browse {
            form.focus_on(path);
            self.open_browser();
        } else if let Some(section) = section {
            form.open_section(section);
        } else if let Some(column) = column {
            form.select_column(column);
        } else if let Some(field) = field {
            form.focus_on(field);
            let action = form.press();
            self.on_import_action(action);
        }
        self.request_probe();
    }
}

fn main_tab_at(tabs: &[MainTab], position: Position) -> Option<MainTab> {
    tabs.iter()
        .copied()
        .find(|tab| position.y == 0 && position.x >= tab.start && position.x < tab.end)
}
