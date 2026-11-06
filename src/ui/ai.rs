//! Completion previews, review controls, and scoped provider settings.
use crate::ai::form::{Field, Form};
use crate::grid::printable;
use crate::ui::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

pub fn draw_form(frame: &mut Frame, form: &Form, theme: &Theme) -> Vec<(Field, Rect)> {
    let area = super::console::centered(
        frame.area(),
        72.min(frame.area().width),
        20.min(frame.area().height),
    );
    frame.render_widget(Clear, area);
    let block = super::float_block(" Autocomplete · IA ", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut targets = Vec::new();
    let scope = match form.scope {
        crate::store::Scope::Global => "Global",
        crate::store::Scope::Project => "Project",
    };
    frame.render_widget(
        Paragraph::new(format!(
            "Scope: {scope}   F1 Provider · F2 Credentials · F3 Limits"
        )),
        Rect { height: 1, ..inner },
    );
    let fields = form.fields();
    let content = fields
        .iter()
        .copied()
        .filter(|f| !matches!(f, Field::Test | Field::Apply | Field::Cancel));
    for (i, field) in content.enumerate() {
        let row = Rect {
            x: inner.x + 1,
            y: inner.y + 2 + i as u16,
            width: inner.width.saturating_sub(2),
            height: 1,
        };
        if row.y >= inner.bottom().saturating_sub(4) {
            break;
        }
        let label = match field {
            Field::Pages => "Page",
            Field::Enabled => "Enabled",
            Field::Profile => "Provider settings",
            Field::Kind => "Provider",
            Field::Model => "Model",
            Field::Endpoint => "Endpoint",
            Field::Protocol => "Protocol",
            Field::Source => "Key source",
            Field::Path => "Key file",
            Field::Key => "New vault key",
            Field::Context => "Context tokens",
            Field::Output => "Output tokens",
            Field::Timeout => "Timeout seconds",
            _ => "",
        };
        let focused = field == form.focus;
        let style = if focused {
            theme.dialog_focus
        } else {
            theme.dialog_option
        };
        let mut spans = vec![Span::styled(
            format!("{} {label:<18} ", if focused { ">" } else { " " }),
            style,
        )];
        let width = usize::from(row.width).saturating_sub(21);
        if let Some(input) = form.input(field) {
            let chars: Vec<_> = if field == Field::Key {
                "*".repeat(input.text().chars().count()).chars().collect()
            } else {
                input.text().chars().collect()
            };
            if chars.is_empty() {
                spans.push(Span::styled(
                    super::console::fit(&printable(&form.value(field)), width).0,
                    style,
                ));
            } else {
                spans.extend(super::input_spans(&chars, input.cursor(), width, focused, theme).0);
            }
        } else {
            spans.push(Span::styled(
                super::console::fit(&printable(&form.value(field)), width).0,
                style,
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
        targets.push((field, row));
    }
    let note = if form.busy {
        "Working… Esc cancels"
    } else {
        form.note
            .as_deref()
            .unwrap_or(if form.focus == Field::Endpoint {
                "Use the API base or full completion URL. Leave blank for the provider default."
            } else {
                "Left/Right changes choices. Apply returns to Settings; Save persists."
            })
    };
    frame.render_widget(
        Paragraph::new(printable(note)).wrap(ratatui::widgets::Wrap { trim: false }),
        Rect {
            x: inner.x,
            y: inner.bottom().saturating_sub(3),
            width: inner.width,
            height: 2,
        },
    );
    let mut x = inner.x + 1;
    for (field, label) in [
        (Field::Test, "Test connection"),
        (Field::Apply, "Apply"),
        (Field::Cancel, "Cancel"),
    ] {
        let label = theme.button(label);
        let width = label.width() as u16;
        let row = Rect::new(x, inner.bottom() - 1, width, 1);
        frame.render_widget(
            Paragraph::new(label).style(if form.focus == field {
                theme.dialog_focus
            } else {
                theme.dialog_option
            }),
            row,
        );
        targets.push((field, row));
        x += width + 2;
    }
    targets
}

pub struct ReviewText {
    lines: Vec<Line<'static>>,
    plain: Vec<String>,
    rows: Vec<(usize, usize, usize)>,
    next: (usize, usize),
    width: usize,
}
impl ReviewText {
    fn new(lines: Vec<Line<'static>>) -> Self {
        let plain = lines.iter().map(Line::to_string).collect();
        Self {
            lines,
            plain,
            rows: Vec::new(),
            next: (0, 0),
            width: 0,
        }
    }
    fn width(&mut self, width: usize) {
        let width = width.max(1);
        if self.width != width {
            self.width = width;
            self.rows.clear();
            self.next = (0, 0);
        }
    }
    fn row(&mut self, index: usize) -> Option<Line<'static>> {
        while self.rows.len() <= index {
            let (line, start) = self.next;
            let text = self.plain.get(line)?;
            let (end, next) = super::wrap::row_end(text, start, self.width);
            self.rows.push((line, start, end));
            self.next = next.map_or((line + 1, 0), |(offset, _)| (line, offset));
        }
        let (line, start, end) = self.rows[index];
        let mut offset = 0;
        let mut spans = Vec::new();
        for span in &self.lines[line].spans {
            let stop = offset + span.content.len();
            let a = start.max(offset);
            let b = end.min(stop);
            if a < b {
                spans.push(Span::styled(
                    span.content[a - offset..b - offset].to_owned(),
                    span.style,
                ));
            }
            offset = stop;
        }
        Some(Line::from(spans))
    }
}

pub fn review_lines(original: &str, proposal: &str, theme: &Theme) -> ReviewText {
    let diff = similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(100))
        .diff_lines(original, proposal);
    let mut lines = Vec::new();
    for group in diff.grouped_ops(3) {
        for op in group {
            for change in diff.iter_changes(&op) {
                let (sign, style) = match change.tag() {
                    similar::ChangeTag::Delete => ("- ", theme.mark_bad),
                    similar::ChangeTag::Insert => ("+ ", theme.mark_good),
                    _ => ("  ", theme.base),
                };
                lines.push(Line::from(vec![Span::styled(
                    format!("{sign}{}", printable(change.value().trim_end_matches('\n'))),
                    style,
                )]));
            }
        }
    }
    // Emphasize changed words in adjacent replacement lines.
    let mut i = 0;
    while i + 1 < lines.len() {
        let old = lines[i].to_string();
        let new = lines[i + 1].to_string();
        if old.starts_with("- ") && new.starts_with("+ ") && old.len() + new.len() < 16384 {
            let words = similar::TextDiff::configure()
                .timeout(std::time::Duration::from_millis(30))
                .diff_words(&old[2..], &new[2..]);
            let mut a = vec![Span::styled("- ", theme.mark_bad)];
            let mut b = vec![Span::styled("+ ", theme.mark_good)];
            for change in words.iter_all_changes() {
                if change.tag() != similar::ChangeTag::Insert {
                    a.push(Span::styled(
                        change.value().to_owned(),
                        if change.tag() == similar::ChangeTag::Delete {
                            theme
                                .mark_bad
                                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                        } else {
                            theme.mark_bad
                        },
                    ));
                }
                if change.tag() != similar::ChangeTag::Delete {
                    b.push(Span::styled(
                        change.value().to_owned(),
                        if change.tag() == similar::ChangeTag::Insert {
                            theme
                                .mark_good
                                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                        } else {
                            theme.mark_good
                        },
                    ));
                }
            }
            lines[i] = Line::from(a);
            lines[i + 1] = Line::from(b);
            i += 1;
        }
        i += 1;
    }
    ReviewText::new(lines)
}

pub fn draw_review(
    frame: &mut Frame,
    area: Rect,
    lines: &mut ReviewText,
    scroll: &mut usize,
    (accept, insertion, reduced): (bool, bool, &str),
    theme: &Theme,
) -> [Rect; 2] {
    let area = if insertion && area.height >= 5 && area.width >= 20 {
        area
    } else {
        super::console::overlay_area(frame.area())
    };
    frame.render_widget(Clear, area);
    let title = if insertion {
        " AI insertion preview "
    } else {
        " AI replacement · Original (-) / Proposal (+) "
    };
    let block = super::float_block(title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let height = inner.height.saturating_sub(2) as usize;
    lines.width(usize::from(inner.width));
    if lines.row(*scroll).is_none() {
        *scroll = lines.rows.len().saturating_sub(1);
    }
    let shown = (*scroll..scroll.saturating_add(height))
        .filter_map(|index| lines.row(index))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(shown),
        Rect {
            height: height as u16,
            ..inner
        },
    );
    frame.render_widget(
        Paragraph::new(reduced).style(theme.hint),
        Rect {
            x: inner.x,
            y: inner.bottom().saturating_sub(2),
            width: inner.width,
            height: 1,
        },
    );
    let mut buttons = [Rect::default(); 2];
    let mut x = inner.x;
    for (index, label) in ["Reject", "Accept"].iter().enumerate() {
        let label = theme.button(label);
        let width = label.width() as u16;
        buttons[index] = Rect::new(x, inner.bottom().saturating_sub(1), width, 1);
        frame.render_widget(
            Paragraph::new(label).style(if (index == 1) == accept {
                theme.dialog_focus
            } else {
                theme.dialog_option
            }),
            buttons[index],
        );
        x += width + 2;
    }
    buttons
}
