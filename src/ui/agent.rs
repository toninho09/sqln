//! Assistant access notifications and scrollable review of proposed SQL edits.

use crate::ui::{draw_overlay, theme::Theme, wrap::WrappedText};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

pub fn draw_access_notice(
    frame: &mut Frame,
    area: Rect,
    pending: usize,
    shortcut: Option<&str>,
    theme: &Theme,
) -> Rect {
    let [label, button] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(8)]).areas(area);
    let label = Rect {
        x: label.x.saturating_add(1),
        width: label.width.saturating_sub(2),
        ..label
    };
    let mut message = format!(
        "MCP: {pending} access request{} pending",
        if pending == 1 { "" } else { "s" }
    );
    if let Some(shortcut) = shortcut
        .filter(|shortcut| message.width() + 2 + shortcut.width() <= usize::from(label.width))
    {
        message.push_str("  ");
        message.push_str(shortcut);
    }
    let (message, _) = super::console::fit(&message, usize::from(label.width));
    frame.render_widget(Paragraph::new(message).style(theme.status_warning), label);
    frame.render_widget(
        Paragraph::new("[Review]").style(theme.dialog_option),
        button,
    );
    button
}

pub struct Review {
    pub proposal: String,
    pub title: String,
    pub contents: WrappedText,
    pub scroll: usize,
    pub accept: bool,
    pub buttons: [Rect; 2],
    pub(crate) _hold: crate::agent::Hold,
}

pub fn draw_review(frame: &mut Frame, review: &mut Review, theme: &Theme) {
    review.scroll = draw_overlay(
        frame,
        &review.title,
        &mut review.contents,
        "Tab: choice | Enter: confirm | Esc: back",
        review.scroll,
        theme,
    );
    let area = super::console::overlay_area(frame.area());
    let y = area.bottom().saturating_sub(1);
    let x = area.x.saturating_add(2);
    review.buttons = [
        Rect::new(x, y, 10, 1),
        Rect::new(x.saturating_add(12), y, 10, 1),
    ];
    for (index, label) in ["[Reject]", "[Accept]"].iter().enumerate() {
        let selected = (index == 1) == review.accept;
        frame.render_widget(
            Paragraph::new(*label).style(if selected {
                theme.dialog_focus
            } else {
                theme.dialog_option
            }),
            review.buttons[index],
        );
    }
}
