//! The title and the rule of a panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use super::super::theme::Theme;
use super::style;

const RULE: &str = "─";

/// Draws the title of a panel. The title of the panel with the focus has its own style.
pub fn draw_panel_title(frame: &mut Frame, area: Rect, title: &str, focused: bool, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(format!(" {title}")).style(style::title_style(focused, theme)),
        area,
    );
}

/// Draws the rule under a row of tabs.
pub fn draw_rule(frame: &mut Frame, area: Rect, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(RULE.repeat(usize::from(area.width))).style(theme.divider),
        area,
    );
}
