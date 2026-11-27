//! The help window: a sidebar with the guide topics and the key groups, a search
//! field, and the page of the selected row with a scrollbar.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::kit::geometry::modal_width;
use super::kit::list::draw_prompt;
use super::kit::modal::{Modal, SidebarSpec};
use super::theme::Theme;
use super::wrap::WrappedText;
use super::{draw_scrollbar, text};
use crate::help::{Help, SidebarRow, View};

fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut contents = WrappedText::new(text.to_owned());
    contents.set_width(width.max(1));
    let mut rows = Vec::new();
    while let Some(row) = contents.row(rows.len()) {
        rows.push(row.to_owned());
    }
    rows
}

/// Draws both columns without cutting a long configured key or description.
fn key_lines(key: &str, label: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let key_width = (width.saturating_sub(2) / 2).clamp(1, 24);
    let label_width = width.saturating_sub(key_width + 2).max(1);
    let keys = wrapped(key, key_width);
    let labels = wrapped(label, label_width);
    (0..keys.len().max(labels.len()))
        .map(|index| {
            let key = keys.get(index).map_or("", String::as_str);
            let label = labels.get(index).cloned().unwrap_or_default();
            Line::from(vec![
                Span::styled(
                    format!("{key}{}", " ".repeat(key_width.saturating_sub(key.width()))),
                    theme.key,
                ),
                Span::raw("  "),
                Span::raw(label),
            ])
        })
        .collect()
}

/// Draws the matching steps and examples of one topic. The title and the intro
/// always show. Returns the lines and the count of matching items.
fn topic_lines(
    help: &Help,
    index: usize,
    width: usize,
    theme: &Theme,
) -> (Vec<Line<'static>>, usize) {
    let topic = &help.guide[index];
    let mut lines = vec![Line::styled(topic.title, theme.panel_title_focused)];
    lines.extend(wrapped(topic.intro, width).into_iter().map(Line::raw));
    let steps: Vec<_> = topic
        .steps
        .iter()
        .filter(|(key, label)| help.search(&format!("{key} {label}")))
        .collect();
    let examples: Vec<_> = topic
        .examples
        .iter()
        .filter(|(label, sql)| help.search(&format!("{label} {sql}")))
        .collect();
    if !steps.is_empty() {
        lines.push(Line::raw(""));
    }
    for (key, label) in &steps {
        lines.extend(key_lines(key, label, width, theme));
    }
    for (label, sql) in &examples {
        lines.push(Line::raw(""));
        lines.extend(wrapped(label, width).into_iter().map(Line::raw));
        let code = wrapped(sql, width.saturating_sub(2));
        lines.extend(
            code.into_iter()
                .map(|row| Line::styled(format!("  {row}"), theme.key)),
        );
    }
    let count = steps.len() + examples.len();
    if count == 0 {
        lines.push(Line::raw(""));
        lines.push(Line::styled(text::NO_ITEMS, theme.hint));
    }
    (lines, count)
}

/// Draws the matching keys of the selected groups. Returns the lines and the count.
fn key_group_lines(help: &Help, width: usize, theme: &Theme) -> (Vec<Line<'static>>, usize) {
    let view = help.view();
    let mut lines = Vec::new();
    let mut count = 0;
    for (index, group) in help.groups.iter().enumerate() {
        if view != View::All && view != View::Group(index) {
            continue;
        }
        let keys: Vec<_> = group
            .keys
            .iter()
            .filter(|(key, label)| help.matches(group, key, label))
            .collect();
        if keys.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(
            format!("{} ({})", group.title, keys.len()),
            theme.panel_title_focused,
        ));
        count += keys.len();
        for (key, label) in keys {
            lines.extend(key_lines(key, label, width, theme));
        }
    }
    if count == 0 {
        lines.push(Line::styled(text::NO_COMMANDS, theme.hint));
    }
    (lines, count)
}

pub fn draw_help(frame: &mut Frame, help: &mut Help, theme: &Theme) {
    let bounds = frame.area();
    let titles = help.page_titles();
    // The sidebar starts below its first row. It keeps the open page in view.
    let sidebar_rows = usize::from(bounds.height.saturating_sub(5)).max(1);
    let first = help.sidebar_window(sidebar_rows);
    let rows: Vec<SidebarRow> = help
        .sidebar_rows()
        .into_iter()
        .skip(first)
        .take(sidebar_rows)
        .collect();
    let names: Vec<String> = rows
        .iter()
        .map(|row| match row {
            SidebarRow::Heading(name) => name.to_uppercase(),
            SidebarRow::Page(page) => titles[*page].to_owned(),
        })
        .collect();
    let shown: Vec<&str> = names.iter().map(String::as_str).collect();
    let headings: Vec<usize> = (0..rows.len())
        .filter(|index| matches!(rows[*index], SidebarRow::Heading(_)))
        .collect();
    let open = rows
        .iter()
        .position(|row| *row == SidebarRow::Page(help.page))
        .unwrap_or(usize::MAX);
    let layout = Modal::sized(
        modal_width(bounds.width, 90),
        bounds.height.saturating_sub(2),
    )
    .title(help.title.trim())
    .sidebar(SidebarSpec {
        titles: &shown,
        open,
        focused: true,
        headings: &headings,
    })
    .open(frame, theme);
    help.page_rows = rows
        .iter()
        .zip(&layout.sidebar_rows)
        .filter_map(|(row, area)| match row {
            SidebarRow::Page(page) => Some((*page, *area)),
            SidebarRow::Heading(_) => None,
        })
        .collect();
    let pane = layout.pane;
    if pane.width < 4 || pane.height < 3 {
        return;
    }
    let prompt = Rect { height: 1, ..pane };
    draw_prompt(frame, &help.input, prompt, theme);
    if help.input.text().is_empty() {
        let search = match help.view() {
            View::Topic(_) => text::HELP_GUIDE_SEARCH,
            _ => text::HELP_SEARCH,
        };
        let hint = Rect {
            x: pane.x + 4,
            width: pane.width.saturating_sub(4),
            ..prompt
        };
        frame.render_widget(Paragraph::new(search).style(theme.hint), hint);
    }
    let content = Rect {
        x: pane.x + 1,
        y: pane.y + 2,
        width: pane.width.saturating_sub(3),
        height: pane.height.saturating_sub(2),
    };
    let width = usize::from(content.width);
    let (lines, count, noun) = match help.view() {
        View::Topic(index) => {
            let (lines, count) = topic_lines(help, index, width, theme);
            (lines, count, "item")
        }
        _ => {
            let (lines, count) = key_group_lines(help, width, theme);
            (lines, count, "command")
        }
    };
    let total = lines.len();
    help.visible_rows = usize::from(content.height).max(1);
    help.scroll = help.scroll.min(total.saturating_sub(help.visible_rows));
    let footer = text::help_footer(
        count,
        noun,
        help.scroll + 1,
        (help.scroll + help.visible_rows).min(total),
        total,
    );
    frame.render_widget(
        Paragraph::new(footer).alignment(Alignment::Right),
        Rect {
            x: layout.area.x + 1,
            y: layout.area.bottom().saturating_sub(1),
            width: layout.area.width.saturating_sub(3),
            height: 1,
        },
    );
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(help.scroll)
                .take(help.visible_rows)
                .collect::<Vec<_>>(),
        ),
        content,
    );
    draw_scrollbar(
        frame.buffer_mut(),
        Rect {
            x: pane.right().saturating_sub(1),
            width: 1,
            ..content
        },
        total,
        help.visible_rows,
        help.scroll,
        theme,
    );
}
