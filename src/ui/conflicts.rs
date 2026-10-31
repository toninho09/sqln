//! The question about the scripts that another program changed.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::console::centered;
use super::text;
use super::theme::Theme;
use crate::grid::printable;

/// What the question shows for one script.
pub struct ConflictRowView<'a> {
    pub name: &'a str,
    pub removed: bool,
    /// The label of the chosen action. `None` waits for the user.
    pub choice: Option<&'static str>,
}

/// The screen areas of the last frame. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct ConflictLayout {
    pub rows: Vec<Rect>,
    /// The areas of **Apply** and **Decide later**.
    pub buttons: [Rect; 2],
}

const WIDTH: u16 = 66;
const NAME_WIDTH: usize = 24;

pub fn draw_conflicts(
    frame: &mut Frame,
    rows: &[ConflictRowView<'_>],
    cursor: usize,
    note: Option<&str>,
    theme: &Theme,
) -> ConflictLayout {
    let height = u16::try_from(rows.len()).unwrap_or(10).min(12) + 6;
    let area = centered(frame.area(), WIDTH, height);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(format!(" {} ", text::TITLE_CONFLICTS)).style(theme.dialog);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut layout = ConflictLayout::default();
    for (index, row) in rows.iter().enumerate().take(usize::from(inner.height.saturating_sub(4))) {
        let line = Rect { y: inner.y + u16::try_from(index).unwrap_or(0), height: 1, ..inner };
        let state = if row.removed { text::STATE_REMOVED } else { text::STATE_CHANGED };
        let name: String = printable(row.name).chars().take(NAME_WIDTH).collect();
        let choice = format!("< {} >", row.choice.unwrap_or(text::CHOICE_NONE));
        let style = if index == cursor { theme.dialog_focus } else { theme.dialog_option };
        let spans = vec![
            Span::raw(format!("{name:<NAME_WIDTH$} {state:<9} ")),
            Span::styled(choice, style),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), line);
        layout.rows.push(line);
    }
    let note_line = Rect { y: inner.bottom().saturating_sub(3), height: 1, ..inner };
    if let Some(note) = note {
        frame.render_widget(Paragraph::new(Line::styled(note, theme.error)), note_line);
    }
    let buttons = Rect { y: inner.bottom().saturating_sub(1), height: 1, ..inner };
    let mut x = buttons.x;
    let mut spans = Vec::new();
    for (index, label) in [text::BUTTON_APPLY, text::BUTTON_LATER].into_iter().enumerate() {
        let name = format!("[{label}]");
        let width = u16::try_from(name.len()).unwrap_or(0);
        layout.buttons[index] = Rect::new(x, buttons.y, width, 1);
        let style = if cursor == rows.len() + index { theme.dialog_focus } else { theme.dialog_option };
        spans.push(Span::styled(name, style));
        spans.push(Span::raw("  "));
        x += width + 2;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), buttons);
    layout
}
