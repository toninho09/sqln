//! The first screen of a console. It has no state, and the first character ends it.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::text;
use super::theme::Theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Welcome {
    pub note: &'static str,
    /// The keys and what each one does.
    pub rows: Vec<(String, &'static str)>,
}

/// Draws over the editor. The first row stays free, because the cursor starts there.
pub fn draw_welcome(frame: &mut Frame, area: Rect, welcome: &Welcome, theme: &Theme) {
    let key_width = welcome.rows.iter().map(|(key, _)| key.width()).max().unwrap_or(0);
    let mut lines = vec![
        Line::styled(text::welcome_title(), theme.welcome_title),
        Line::styled(welcome.note, theme.hint),
        Line::raw(""),
    ];
    for (key, label) in &welcome.rows {
        lines.push(Line::from(vec![
            Span::styled(format!("{key:>key_width$}"), theme.key),
            Span::styled(format!("  {label}"), theme.hint),
        ]));
    }
    let width = u16::try_from(lines.iter().map(Line::width).max().unwrap_or(0)).unwrap_or(u16::MAX);
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let free = area.height.saturating_sub(1);
    let y = area.y + 1 + free.saturating_sub(height) / 2;
    let x = area.x + area.width.saturating_sub(width) / 2;
    let place = Rect::new(x, y, width.min(area.width), height.min(free));
    frame.render_widget(Paragraph::new(lines), place);
}
