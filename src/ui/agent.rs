//! The MCP row, and the scrollable review of proposed SQL edits.

use crate::ui::{console::draw_overlay_styled, text, theme::Theme, wrap::WrappedText};
use ratatui::{Frame, layout::Rect, style::Style, text::Span, widgets::Paragraph};
use unicode_width::UnicodeWidthStr;

/// The content of the MCP row.
pub struct Bar {
    /// The long and the short text of the pending items.
    pub items: (String, String),
    /// The effective key of the review, such as `F8: sessions`.
    pub shortcut: Option<String>,
    pub review: bool,
    pub stop: bool,
}

/// The click targets of the MCP row.
#[derive(Clone, Copy, Debug, Default)]
pub struct BarTargets {
    pub review: Rect,
    pub stop: Rect,
}

/// The card of the most urgent agent item: an access request, an edit proposal, or a
/// long task. The user did not put it off.
pub struct Card {
    pub title: &'static str,
    pub message: String,
    /// The effective key of a command for the item, such as `F8: sessions`.
    pub shortcut: Option<String>,
    /// The button that acts, such as Review or Stop.
    pub primary: &'static str,
    /// The button that hides the card, such as Later or Hide.
    pub secondary: &'static str,
}

/// The click targets of a card. `area` takes the clicks outside the buttons.
#[derive(Clone, Copy, Debug, Default)]
pub struct CardTargets {
    pub area: Rect,
    pub primary: Rect,
    pub secondary: Rect,
}

/// The widest access card, with its border.
const CARD_WIDTH: u16 = 56;
/// The narrowest access card. A smaller area keeps only the MCP row.
const CARD_MIN_WIDTH: u16 = 36;

/// Draws the card at the bottom right of `area`, next to the MCP row.
/// The editor text and its cursor stay visible. The card does not take the focus.
pub fn draw_card(frame: &mut Frame, area: Rect, card: &Card, theme: &Theme) -> CardTargets {
    let width = CARD_WIDTH.min(area.width.saturating_sub(2));
    if width < CARD_MIN_WIDTH || area.height < 6 {
        return CardTargets::default();
    }
    let rect = Rect::new(area.right() - width - 1, area.bottom() - 4, width, 4);
    frame.render_widget(ratatui::widgets::Clear, rect);
    let warning = Style {
        bg: None,
        ..theme.status_warning
    };
    let block = super::float_block(card.title, theme)
        .border_style(warning)
        .title_style(warning)
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let room = usize::from(inner.width);
    let (message, _) = super::console::fit(&card.message, room);
    frame.render_widget(Paragraph::new(message), Rect { height: 1, ..inner });
    let row = Rect {
        y: inner.y + 1,
        height: 1,
        ..inner
    };
    let mut right = row.right();
    let mut place = |label: &str| {
        let label = theme.button(label);
        let width = u16::try_from(label.width()).unwrap_or(u16::MAX);
        let x = right.saturating_sub(width).max(row.x);
        let target = Rect::new(x, row.y, right.saturating_sub(x), 1);
        frame.render_widget(Paragraph::new(label).style(theme.dialog_option), target);
        right = x.saturating_sub(2);
        target
    };
    let secondary = place(card.secondary);
    let primary = place(card.primary);
    if let Some(shortcut) = card
        .shortcut
        .as_ref()
        .filter(|shortcut| shortcut.width() + 1 < usize::from(right.saturating_sub(row.x)))
    {
        frame.render_widget(Paragraph::new(shortcut.as_str()), row);
    }
    CardTargets {
        area: rect,
        primary,
        secondary,
    }
}

/// Draws the MCP row. The buttons stay at the right, and the text gives way to them.
pub fn draw_bar(frame: &mut Frame, area: Rect, bar: &Bar, theme: &Theme) -> BarTargets {
    let mut targets = BarTargets::default();
    let mut right = area.right().saturating_sub(1);
    let mut place = |label: &str| {
        let label = theme.button(label);
        let width = u16::try_from(label.width()).unwrap_or(u16::MAX);
        let x = right.saturating_sub(width).max(area.x);
        let rect = Rect::new(x, area.y, right.saturating_sub(x), 1);
        frame.render_widget(Paragraph::new(label).style(theme.dialog_option), rect);
        right = x.saturating_sub(2);
        rect
    };
    if bar.review {
        targets.review = place(text::AGENT_REVIEW);
    }
    if bar.stop {
        targets.stop = place(text::AGENT_STOP);
    }
    let label = " MCP ";
    let room = usize::from(right.saturating_sub(area.x)).saturating_sub(label.width() + 1);
    let (long, short) = &bar.items;
    let mut message = if long.width() <= room { long } else { short }.clone();
    if let Some(shortcut) = bar
        .shortcut
        .as_ref()
        .filter(|shortcut| message.width() + 2 + shortcut.width() <= room)
    {
        message.push_str("  ");
        message.push_str(shortcut);
    }
    let (message, _) = super::console::fit(&message, room);
    let line = ratatui::text::Line::from(vec![
        Span::styled(label, theme.status_warning),
        Span::raw(" "),
        Span::raw(message.to_owned()),
    ]);
    let left = Rect {
        width: right.saturating_sub(area.x),
        ..area
    };
    frame.render_widget(Paragraph::new(line), left);
    targets
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

/// Counts the added and the removed lines of a unified diff.
pub fn diff_counts(diff: &str) -> (usize, usize) {
    diff.lines().fold((0, 0), |(added, removed), line| {
        if line.starts_with('+') && !line.starts_with("+++") {
            (added + 1, removed)
        } else if line.starts_with('-') && !line.starts_with("---") {
            (added, removed + 1)
        } else {
            (added, removed)
        }
    })
}

/// Returns the style of a diff line from its first characters.
fn diff_style(head: &str, theme: &Theme) -> Style {
    if head.starts_with("+++") || head.starts_with("---") || head.starts_with("@@") {
        theme.hint
    } else if head.starts_with('+') {
        theme.mark_good
    } else if head.starts_with('-') {
        theme.mark_bad
    } else {
        Style::new()
    }
}

pub fn draw_review(frame: &mut Frame, review: &mut Review, theme: &Theme) {
    review.scroll = draw_overlay_styled(
        frame,
        &review.title,
        &mut review.contents,
        text::AGENT_REVIEW_FOOTER,
        review.scroll,
        theme,
        |head| diff_style(head, theme),
    );
    let area = super::console::overlay_area(frame.area());
    let y = area.bottom().saturating_sub(1);
    let mut x = area.x.saturating_add(2);
    for (index, label) in [text::AGENT_REJECT, text::AGENT_ACCEPT].iter().enumerate() {
        let label = theme.button(label);
        let width = u16::try_from(label.width()).unwrap_or(u16::MAX);
        let selected = (index == 1) == review.accept;
        review.buttons[index] = Rect::new(x, y, width, 1).intersection(area);
        frame.render_widget(
            Paragraph::new(label).style(if selected {
                theme.dialog_focus
            } else {
                theme.dialog_option
            }),
            review.buttons[index],
        );
        x = x.saturating_add(width + 2);
    }
}
