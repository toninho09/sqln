//! The grouped key reference, with a search field and a scrollbar.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::console::{centered, modal_width};
use super::palette::draw_prompt;
use super::theme::Theme;
use super::wrap::WrappedText;
use super::{draw_scrollbar, float_block, text};
use crate::help::Help;

fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut contents = WrappedText::new(text.to_owned());
    contents.set_width(width);
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

pub fn draw_help(frame: &mut Frame, help: &mut Help, theme: &Theme) {
    let bounds = frame.area();
    let area = centered(
        bounds,
        modal_width(bounds.width, 72),
        bounds.height.saturating_sub(2),
    );
    frame.render_widget(Clear, area);
    let block = float_block(help.title.trim(), theme);
    let inner = block.inner(area);
    if inner.width < 4 || inner.height < 3 {
        frame.render_widget(block, area);
        return;
    }
    let content = Rect {
        x: inner.x + 1,
        y: inner.y + 2,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(2),
    };
    draw_prompt(frame, &help.input, Rect { height: 1, ..inner }, theme);
    if help.input.text().is_empty() {
        let hint = Rect {
            x: inner.x + 4,
            width: inner.width.saturating_sub(4),
            height: 1,
            ..inner
        };
        frame.render_widget(Paragraph::new(text::HELP_SEARCH).style(theme.hint), hint);
    }
    let group = help
        .group
        .map_or(text::HELP_ALL, |index| help.groups[index].title);
    let group_line = Line::from(vec![
        Span::styled(
            format!(" {}: {group}", text::HELP_GROUP),
            theme.panel_title_focused,
        ),
        Span::styled(format!("  · {}", text::HELP_GROUP_KEYS), theme.hint),
    ]);
    frame.render_widget(
        Paragraph::new(group_line),
        Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        },
    );

    let mut lines = Vec::new();
    let mut count = 0;
    for (index, group) in help.groups.iter().enumerate() {
        if help.group.is_some_and(|selected| selected != index) {
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
            lines.extend(key_lines(key, label, usize::from(content.width), theme));
        }
    }
    if count == 0 {
        lines.push(Line::styled(text::NO_COMMANDS, theme.hint));
    }
    let total = lines.len();
    help.visible_rows = usize::from(content.height).max(1);
    help.scroll = help.scroll.min(total.saturating_sub(help.visible_rows));
    let footer = text::help_footer(
        count,
        help.scroll + 1,
        (help.scroll + help.visible_rows).min(total),
        total,
    );
    frame.render_widget(
        block.title_bottom(Line::raw(footer).alignment(Alignment::Right)),
        area,
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
            x: inner.right().saturating_sub(1),
            width: 1,
            ..content
        },
        total,
        help.visible_rows,
        help.scroll,
        theme,
    );
}
