//! The mouse: clicks, the wheel, and the drag of dividers and columns.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::{App, MAX_EDITOR_RATIO, MIN_EDITOR_RATIO};
use crate::console::Focus;
use crate::form::FormRequest;
use crate::input::Action;
use crate::navigator::{MIN_WIDTH, Navigator};
use crate::ui::console::Control;
use crate::ui::{column_edge_at, grid_cell_at, tab_at};

/// The console keeps this width when the user drags the divider of the navigator.
pub(super) const MIN_MAIN_WIDTH: u16 = 40;
const WHEEL_ROWS: isize = 3;

#[derive(Clone, Copy)]
pub(super) enum Drag {
    /// The edge of a column in the grid header.
    Column { column: usize, start: u16, width: usize },
    Navigator,
    Editor,
}

impl App {
    pub(super) fn on_mouse(&mut self, mouse: MouseEvent) {
        if self.form.is_some() {
            self.on_form_mouse(mouse);
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.on_press(mouse, position),
            MouseEventKind::Drag(MouseButton::Left) if self.drag.is_some() => self.on_drag(mouse),
            MouseEventKind::Up(_) if self.drag.is_some() => self.drag = None,
            MouseEventKind::Drag(MouseButton::Left)
                if !self.navigator_focused && self.console().focus == Focus::Results =>
            {
                self.point_at_cell(mouse, true);
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => self.on_wheel(mouse, position),
            _ if self.layout.console.editor.contains(position) && self.editor_has_focus() => {
                self.console_mut().editor.on_mouse(mouse);
            }
            _ => {}
        }
    }

    fn editor_has_focus(&self) -> bool {
        !self.navigator_focused && self.console().focus == Focus::Editor
    }

    fn on_press(&mut self, mouse: MouseEvent, position: Position) {
        let layout = &self.layout;
        let in_tabs = layout.console.tabs.contains(position);
        let in_results = layout.console.results.contains(position);
        let in_editor = layout.console.editor.contains(position);
        let in_controls = layout.console.controls.contains(position);
        let in_navigator = layout.navigator.contains(position);
        if layout.divider == Some(mouse.column) && layout.body.contains(position) {
            self.drag = Some(Drag::Navigator);
        } else if let Some(tab) = main_tab_at(&layout.main_tabs, position) {
            self.select_console(tab);
        } else if in_navigator {
            self.on_navigator_press(mouse);
        } else if in_controls {
            self.on_control_press(mouse.column);
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

    fn on_navigator_press(&mut self, mouse: MouseEvent) {
        self.navigator_focused = true;
        let area = self.layout.navigator;
        let index = self.navigator.top + usize::from(mouse.row - area.y);
        let rows = self.navigator.rows(&self.connections);
        let Some(row) = rows.get(index).copied() else { return };
        self.navigator.select_index(index, &self.connections);
        // The mark of a branch stands after the indent of its row.
        let mark = usize::from(area.x) + row.depth() * 2;
        if usize::from(mouse.column) == mark && Navigator::is_branch(&row) {
            self.toggle(&row);
        }
    }

    fn on_control_press(&mut self, column: u16) {
        let control = self.layout.console.control_buttons.iter().find(|button| {
            column >= button.0 && column < button.1
        });
        let Some((_, _, control)) = control else { return };
        let action = match control {
            Control::Run => Action::Run,
            Control::Script => Action::RunScript,
            Control::Stop => Action::Stop,
            Control::Mode => Action::ToggleMode,
            Control::Commit => Action::Commit,
            Control::Rollback => Action::Rollback,
            Control::Schema => Action::ChooseSchema,
        };
        self.perform(action, false);
    }

    fn on_wheel(&mut self, mouse: MouseEvent, position: Position) {
        let rows = if mouse.kind == MouseEventKind::ScrollUp { -WHEEL_ROWS } else { WHEEL_ROWS };
        if self.layout.navigator.contains(position) {
            self.navigator.move_by(rows, &self.connections);
        } else if self.layout.console.results.contains(position) {
            let console = self.console_mut();
            let active = console.active;
            if let Some(result) = console.results.get_mut(active) {
                result.view.move_by(rows, 0, &result.table, false);
            }
        } else if self.layout.console.editor.contains(position) && self.editor_has_focus() {
            self.console_mut().editor.on_mouse(mouse);
        }
    }

    fn on_drag(&mut self, mouse: MouseEvent) {
        match self.drag {
            Some(Drag::Column { .. }) => self.drag_column(mouse),
            Some(Drag::Navigator) => {
                let start = self.layout.body.x;
                let limit = self.layout.body.width.saturating_sub(MIN_MAIN_WIDTH);
                let width = mouse.column.saturating_sub(start);
                self.navigator.width = width.clamp(MIN_WIDTH.min(limit), limit);
            }
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
        let console = self.console();
        let Some(result) = console.results.get(console.active) else { return false };
        let area = self.layout.console.results;
        self.drag = column_edge_at(result, area, mouse.column, mouse.row).map(|column| {
            Drag::Column { column, start: mouse.column, width: result.table.width(column) }
        });
        self.drag.is_some()
    }

    fn drag_column(&mut self, mouse: MouseEvent) {
        let Some(Drag::Column { column, start, width }) = self.drag else { return };
        let console = self.console_mut();
        let active = console.active;
        let Some(result) = console.results.get_mut(active) else { return };
        let change = isize::from(mouse.column as i16 - start as i16);
        result.table.set_width(column, width.saturating_add_signed(change));
    }

    fn point_at_cell(&mut self, mouse: MouseEvent, extend: bool) {
        let area = self.layout.console.results;
        let console = self.console_mut();
        let active = console.active;
        let Some(result) = console.results.get_mut(active) else { return };
        if let Some((row, col)) = grid_cell_at(result, area, mouse.column, mouse.row) {
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
            .find(|(start, end, _)| mouse.row == *row && mouse.column >= *start && mouse.column < *end)
            .map(|(_, _, index)| *index);
        let Some(index) = option else { return };
        if let Some(dialog) = self.dialogs.pop_front() {
            self.on_dialog_action(dialog.options[index].1);
        }
    }

    pub(super) fn on_picker_mouse(&mut self, mouse: MouseEvent) {
        let (area, first) = self.layout.picker;
        let inside = area.contains(Position::new(mouse.column, mouse.row));
        let Some(picker) = &mut self.picker else { return };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let index = first + usize::from(mouse.row - area.y);
                if index <= picker.items.len() {
                    picker.cursor = index;
                    self.accept_picker();
                }
            }
            MouseEventKind::ScrollUp => picker.cursor = picker.cursor.saturating_sub(1),
            MouseEventKind::ScrollDown => picker.cursor = (picker.cursor + 1).min(picker.items.len()),
            _ => {}
        }
    }

    fn on_form_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        let field = self.layout.form.fields.iter().find(|(_, area)| area.contains(position));
        let Some((field, _)) = field else { return };
        let field = *field;
        let Some(form) = &mut self.form else { return };
        form.focus_on(field);
        let request = form.press();
        match request {
            FormRequest::Test => self.test_form(),
            FormRequest::Save => self.save_form(),
            FormRequest::Disconnect => self.disconnect_form(),
            FormRequest::Cancel => self.form = None,
            FormRequest::None => {}
        }
    }
}

fn main_tab_at(tabs: &[(u16, u16, usize)], position: Position) -> Option<usize> {
    tabs.iter()
        .find(|(start, end, _)| position.y == 0 && position.x >= *start && position.x < *end)
        .map(|(_, _, index)| *index)
}
