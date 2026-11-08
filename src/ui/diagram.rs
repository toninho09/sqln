//! The diagram view: the title, the picture of the boxes and the lines, and the rows of
//! the state.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::kit::feedback::message_style;
use super::kit::measure::fit;
use super::kit::panel::draw_panel_title;
use super::text;
use super::theme::Theme;
use crate::console::Phase;
use crate::diagram::Focus;
use crate::diagram::layout::{Placed, glyph, stub_name};
use crate::diagram::tab::DiagramTab;
use crate::grid::printable;

pub struct DiagramBar<'a> {
    pub connection: &'a str,
    /// True on a tall screen, which has a title row.
    pub title: bool,
}

/// Writes the label of a diagram tab.
pub fn tab_label(tab: &DiagramTab, theme: &Theme) -> String {
    super::main_tab_label(theme.icons.schema, &tab.label(), false)
}

/// The part of the picture on screen.
struct View<'a> {
    buffer: &'a mut Buffer,
    area: Rect,
    left: u32,
    top: u32,
}

impl View<'_> {
    /// Writes text from a cell of the picture. The part out of the view stays out.
    fn text(&mut self, x: u32, y: u32, text: &str, style: Style) {
        let Some(row) = y
            .checked_sub(self.top)
            .filter(|row| *row < u32::from(self.area.height))
        else {
            return;
        };
        let mut column = x;
        for c in text.chars() {
            let width = c.width().unwrap_or(0) as u32;
            let start = column.checked_sub(self.left);
            if let Some(start) = start.filter(|start| start + width <= u32::from(self.area.width)) {
                let position = (self.area.x + start as u16, self.area.y + row as u16);
                self.buffer.set_stringn(
                    position.0,
                    position.1,
                    c.to_string(),
                    width as usize,
                    style,
                );
            }
            column += width;
        }
    }

    fn holds(&self, placed: &Placed) -> bool {
        let right = self.left + u32::from(self.area.width);
        let bottom = self.top + u32::from(self.area.height);
        placed.x < right
            && placed.x + placed.width > self.left
            && placed.y < bottom
            && placed.y + placed.height > self.top
    }
}

/// Draws the diagram view. It returns the area of the picture, for the mouse.
pub fn draw_diagram(
    frame: &mut Frame,
    area: Rect,
    tab: &mut DiagramTab,
    bar: &DiagramBar,
    focused: bool,
    theme: &Theme,
) -> Rect {
    let [title, picture, summary, notice] = Layout::vertical([
        Constraint::Length(u16::from(bar.title)),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    if bar.title {
        let name = match &tab.focus {
            Focus::Schema(schema) => schema.clone(),
            Focus::Table(table) => format!("{}.{}", table.schema, table.relation),
        };
        let heading = text::diagram_title(bar.connection, &name);
        draw_panel_title(frame, title, &printable(&heading), focused, theme);
    }
    let icons = &theme.icons;
    let marks = icons.primary_key.width().max(icons.foreign_key.width());
    tab.set_mark_width(marks as u32);
    tab.set_view(picture.width, picture.height);
    if tab.model.entities.is_empty() {
        draw_empty(frame, picture, tab, theme);
    } else {
        draw_picture(frame.buffer_mut(), picture, tab, theme);
    }
    let line = Line::styled(format!(" {}", summary_text(tab)), theme.hint);
    frame.render_widget(Paragraph::new(line), summary);
    if let Some(message) = tab
        .message
        .as_ref()
        .filter(|_| !tab.model.entities.is_empty())
    {
        let line = Line::styled(
            format!(" {}", printable(&message.text)),
            message_style(message, theme),
        );
        frame.render_widget(Paragraph::new(line), notice);
    }
    picture
}

fn summary_text(tab: &DiagramTab) -> String {
    let model = &tab.model;
    let mut parts = vec![text::diagram_summary(
        model.entities.len(),
        model.links.len(),
    )];
    match tab.phase {
        Phase::Waiting => parts.push(text::TABLE_WAITING.to_owned()),
        Phase::Running => parts.push(text::TABLE_LOADING.to_owned()),
        Phase::Stopping => parts.push(text::TABLE_STOPPING.to_owned()),
        Phase::Idle => {}
    }
    if tab.keys_only {
        parts.push(text::DIAGRAM_KEYS_ONLY.to_owned());
    }
    if tab.partial {
        parts.push(text::DIAGRAM_PARTIAL.to_owned());
    }
    parts.join(" · ")
}

/// Draws the area of a diagram without boxes: the reason, or the wait.
fn draw_empty(frame: &mut Frame, area: Rect, tab: &DiagramTab, theme: &Theme) {
    let line = match &tab.message {
        Some(message) => Line::styled(printable(&message.text), message_style(message, theme)),
        None if tab.is_busy() => Line::styled(text::TABLE_LOADING, theme.hint),
        None => return,
    };
    frame.render_widget(Paragraph::new(line).wrap(Wrap { trim: false }), area);
}

fn draw_picture(buffer: &mut Buffer, area: Rect, tab: &DiagramTab, theme: &Theme) {
    let mut view = View {
        buffer,
        area,
        left: tab.left,
        top: tab.top,
    };
    let layout = &tab.layout;
    let icons = &theme.icons;
    for (entity, placed) in layout.boxes.iter().enumerate() {
        if view.holds(placed) {
            draw_box(&mut view, tab, entity, theme);
        }
    }
    let line_style = |mut links: std::slice::Iter<usize>| {
        let chosen = tab
            .selected
            .is_some_and(|entity| links.any(|&link| tab.model.links[link].touches(entity)));
        if chosen {
            theme.panel_title_focused
        } else {
            theme.divider
        }
    };
    let right = view.left + u32::from(area.width);
    for y in view.top..view.top + u32::from(area.height) {
        for (x, arms, pieces) in layout.cells(y) {
            if x >= view.left && x < right {
                let links: Vec<usize> = pieces.iter().map(|piece| piece.link).collect();
                view.text(x, y, &glyph(arms).to_string(), line_style(links.iter()));
            }
        }
    }
    for arrow in &layout.arrows {
        let mark = if arrow.left {
            icons.arrow_left
        } else {
            icons.arrow_right
        };
        view.text(arrow.x, arrow.y, mark, line_style([arrow.link].iter()));
    }
}

fn draw_box(view: &mut View, tab: &DiagramTab, entity: usize, theme: &Theme) {
    let placed = &tab.layout.boxes[entity];
    let item = &tab.model.entities[entity];
    let chosen = tab.selected == Some(entity);
    let border = match (chosen, item.stub) {
        (true, _) => theme.panel_title_focused,
        (false, true) => theme.comment,
        (false, false) => theme.float_border,
    };
    let (x, y, width, height) = (placed.x, placed.y, placed.width, placed.height);
    let inner = width.saturating_sub(2) as usize;
    let rule = "─".repeat(inner);
    view.text(x, y, &format!("┌{rule}┐"), border);
    for row in y + 1..y + height - 1 {
        view.text(x, row, "│", border);
        view.text(x + 1, row, &" ".repeat(inner), theme.base);
        view.text(x + width - 1, row, "│", border);
    }
    view.text(x, y + height - 1, &format!("└{rule}┘"), border);
    let room = inner.saturating_sub(2);
    if item.stub {
        let name = fit(&printable(&stub_name(item)), room).0;
        view.text(x + 2, y + 1, &name, theme.comment);
        return;
    }
    let title = format!(" {} ", fit(&printable(&item.relation.relation), room).0);
    let title_style = if chosen {
        theme.tree_selected
    } else {
        theme.float_title
    };
    view.text(x + 1, y, &title, title_style);

    let icons = &theme.icons;
    let mark_width = icons.primary_key.width().max(icons.foreign_key.width());
    let name_width = placed.name_width as usize;
    let type_width = inner.saturating_sub(mark_width + name_width + 5);
    for (place, &column) in placed.rows.iter().enumerate() {
        let column = &item.columns[column];
        let row = y + 1 + place as u32;
        let (mark, style) = if column.primary.is_some() {
            (icons.primary_key, theme.status)
        } else if column.foreign {
            (icons.foreign_key, theme.control)
        } else {
            ("", theme.base)
        };
        let mut at = x + 2;
        view.text(at, row, mark, style);
        at += mark_width as u32 + 1;
        view.text(
            at,
            row,
            &fit(&printable(&column.name), name_width).0,
            theme.base,
        );
        at += name_width as u32 + 2;
        view.text(
            at,
            row,
            &fit(&printable(&column.ty), type_width).0,
            theme.hint,
        );
    }
}
