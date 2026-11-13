//! The first screen of a console, and the home screen. Neither has state, and the
//! first character ends the first screen of a console.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::text;
use super::theme::Theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Welcome {
    /// True for the home screen: the logo replaces the name when the area has room.
    pub logo: bool,
    pub note: &'static str,
    /// The keys and what each one does.
    pub rows: Vec<(String, &'static str)>,
}

/// Draws over the editor. The first row stays free, because the cursor starts there.
pub fn draw_welcome(frame: &mut Frame, area: Rect, welcome: &Welcome, theme: &Theme) {
    let key_width = welcome
        .rows
        .iter()
        .map(|(key, _)| key.width())
        .max()
        .unwrap_or(0);
    let free = area.height.saturating_sub(1);
    // The logo and the version take the place of the name, with the note, a blank row,
    // and the rows of keys under them.
    let logo = welcome.logo && usize::from(free) >= text::HOME_LOGO.len() + 3 + welcome.rows.len();
    let mut lines = Vec::new();
    let header_rows = usize::from(free).saturating_sub(welcome.rows.len());
    if logo {
        for row in text::HOME_LOGO {
            lines.push(Line::styled(row, theme.welcome_title).centered());
        }
        lines.push(Line::styled(text::HOME_VERSION, theme.hint).centered());
        lines.push(Line::styled(welcome.note, theme.hint).centered());
    } else {
        if header_rows > 0 {
            lines.push(Line::styled(text::welcome_title(), theme.welcome_title));
        }
        if header_rows > 1 {
            lines.push(Line::styled(welcome.note, theme.hint));
        }
    }
    if logo || header_rows > 2 {
        lines.push(Line::raw(""));
    }
    for (key, label) in &welcome.rows {
        if key.is_empty() {
            lines.push(Line::styled(*label, theme.welcome_title));
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}{key}", " ".repeat(key_width - key.width())),
                    theme.key,
                ),
                Span::styled(format!("  {label}"), theme.hint),
            ]));
        }
    }
    let width = u16::try_from(lines.iter().map(Line::width).max().unwrap_or(0)).unwrap_or(u16::MAX);
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let y = area.y + 1 + free.saturating_sub(height) / 2;
    let x = area.x + area.width.saturating_sub(width) / 2;
    let place = Rect::new(x, y, width.min(area.width), height.min(free));
    frame.render_widget(Paragraph::new(lines), place);
}
