//! The parameter panel below the SQL editor.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::kit::measure::fit;
use super::kit::panel::draw_panel_title;
use super::theme::Theme;
use super::{draw_scrollbar, text};
use crate::grid::{printable, printable_start};
use crate::line_input::LineInput;
use crate::parameters::Panel;
use crate::sql::parameters::Kind;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParameterLayout {
    pub header: Rect,
    pub rows: Vec<(usize, [Rect; 3])>,
    pub area: Rect,
}

pub fn height(panel: &Panel, available: u16) -> u16 {
    if panel.fields.is_empty() {
        0
    } else if panel.collapsed {
        1.min(available)
    } else {
        (panel
            .fields
            .len()
            .saturating_add(1 + usize::from(panel.error.is_some()))
            .min(6) as u16)
            .min(available.saturating_sub(5) / 2)
            .max(1)
            .min(available)
    }
}

pub(crate) fn input_start(input: &LineInput, width: usize) -> usize {
    let mut used = 1;
    let mut start = input.byte_cursor();
    for (index, c) in input.text()[..start].char_indices().rev() {
        used += c.width().unwrap_or(0);
        if used > width {
            break;
        }
        start = index;
    }
    start
}

fn input_line(input: &LineInput, width: usize, focused: bool, theme: &Theme) -> Line<'static> {
    let text = input.text();
    if !focused {
        return Line::raw(printable_start(text, width));
    }
    let cursor = input.byte_cursor();
    let before = &text[..cursor];
    let start = input_start(input, width);
    let prefix = printable(&before[start..]);
    let after = &text[cursor..];
    let next = after.chars().next();
    let at = next.map_or_else(|| " ".to_owned(), |c| printable(&c.to_string()));
    let rest = &after[next.map_or(0, char::len_utf8)..];
    let free = width.saturating_sub(prefix.width() + at.width());
    Line::from(vec![
        Span::raw(prefix),
        Span::styled(at, theme.cursor),
        Span::raw(printable_start(rest, free)),
    ])
}

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    panel: &mut Panel,
    focused: bool,
    shortcut: &str,
    theme: &Theme,
) -> ParameterLayout {
    if area.height == 0 {
        return ParameterLayout::default();
    }
    let header = Rect { height: 1, ..area };
    let state = if panel.collapsed { "+" } else { "-" };
    let shortcut = if shortcut.is_empty() {
        String::new()
    } else {
        format!(" · {shortcut}")
    };
    let title = format!(
        "[{state}] {} ({}){shortcut}",
        text::PARAMETERS_TITLE,
        panel.fields.len()
    );
    draw_panel_title(frame, header, &title, focused, theme);
    let mut layout = ParameterLayout {
        header,
        rows: Vec::new(),
        area,
    };
    if panel.collapsed {
        return layout;
    }
    let error_rows = u16::from(panel.error.is_some() && area.height > 2);
    if let Some(error) = panel.error.as_ref().filter(|_| error_rows > 0) {
        let note = Rect {
            y: area.bottom() - 1,
            height: 1,
            ..area
        };
        frame.render_widget(
            Paragraph::new(fit(&printable(error), note.width.into()).0).style(theme.error),
            note,
        );
    }
    let rows = usize::from(area.height.saturating_sub(1 + error_rows));
    if rows == 0 {
        return layout;
    }
    panel.scroll = panel.scroll.min(panel.selected);
    if panel.selected >= panel.scroll + rows {
        panel.scroll = panel.selected + 1 - rows;
    }
    let names = 18.min(area.width / 3);
    for (visible, (index, field)) in panel
        .fields
        .iter()
        .enumerate()
        .skip(panel.scroll)
        .take(rows)
        .enumerate()
    {
        let row = Rect {
            y: area.y + 1 + visible as u16,
            height: 1,
            ..area
        };
        let [label, kind, value, set, _scroll] = Layout::horizontal([
            Constraint::Length(names),
            Constraint::Length(9),
            Constraint::Min(1),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .areas(row);
        let selected = focused && panel.selected == index;
        let style = if field.invalid {
            theme.error
        } else {
            theme.hint
        };
        frame.render_widget(
            Paragraph::new(
                fit(
                    &format!("{}:{}", if field.invalid { "!" } else { " " }, field.name),
                    label.width.into(),
                )
                .0,
            )
            .style(style),
            label,
        );
        let kind_index = match field.kind {
            Kind::Text => 0,
            Kind::Number => 1,
            Kind::Boolean => 2,
            Kind::Null => 3,
        };
        let kind_style = if selected && panel.column == 0 {
            theme.field_focus
        } else {
            theme.field
        };
        frame.render_widget(
            Paragraph::new(format!("[{}]", text::PARAMETER_TYPES[kind_index])).style(kind_style),
            kind,
        );
        let value_style = if selected && panel.column == 1 {
            theme.field_focus
        } else {
            theme.field
        };
        let content = if field.kind == Kind::Null {
            Line::raw("NULL")
        } else {
            input_line(
                &field.input,
                value.width.into(),
                selected && panel.column == 1,
                theme,
            )
        };
        frame.render_widget(Paragraph::new(content).style(value_style), value);
        let set_style = if selected && panel.column == 2 {
            theme.field_focus
        } else if field.invalid {
            theme.error
        } else {
            theme.field
        };
        let label = if field.set {
            text::PARAMETER_SET
        } else {
            text::PARAMETER_UNSET
        };
        frame.render_widget(Paragraph::new(format!("[{label}]")).style(set_style), set);
        layout.rows.push((index, [kind, value, set]));
    }
    draw_scrollbar(
        frame.buffer_mut(),
        Rect {
            x: area.right().saturating_sub(1),
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(1),
        },
        panel.fields.len(),
        rows,
        panel.scroll,
        theme,
    );
    layout
}
