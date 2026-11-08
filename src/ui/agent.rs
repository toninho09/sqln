//! The MCP row, and the scrollable review of proposed SQL edits.

use super::kit::buttons::{self, ButtonState, RowOptions};
use crate::ui::{text, theme::Theme};
use ratatui::{Frame, layout::Rect, text::Span, widgets::Paragraph};
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
    let block = super::kit::modal::warning_block(card.title, theme);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let room = usize::from(inner.width);
    let (message, _) = super::kit::measure::fit(&card.message, room);
    frame.render_widget(Paragraph::new(message), Rect { height: 1, ..inner });
    let row = Rect {
        y: inner.y + 1,
        height: 1,
        ..inner
    };
    let items = [
        (0usize, theme.button(card.primary), ButtonState::Idle),
        (1usize, theme.button(card.secondary), ButtonState::Idle),
    ];
    let drawn = buttons::draw(frame, row, RowOptions::INLINE_RIGHT, &items, theme);
    let target = |index: usize| {
        drawn
            .iter()
            .find(|(id, _)| *id == index)
            .map_or(Rect::default(), |(_, rect)| *rect)
    };
    let primary = target(0);
    let secondary = target(1);
    let right = drawn
        .iter()
        .map(|(_, rect)| rect.x)
        .min()
        .map_or(row.right(), |x| x.saturating_sub(2));
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
    let mut items = Vec::new();
    if bar.stop {
        items.push((0usize, theme.button(text::AGENT_STOP), ButtonState::Idle));
    }
    if bar.review {
        items.push((1usize, theme.button(text::AGENT_REVIEW), ButtonState::Idle));
    }
    let buttons_area = Rect {
        width: area.width.saturating_sub(1),
        height: 1,
        ..area
    };
    let drawn = buttons::draw(frame, buttons_area, RowOptions::INLINE_RIGHT, &items, theme);
    for (id, rect) in &drawn {
        if *id == 0 {
            targets.stop = *rect;
        } else {
            targets.review = *rect;
        }
    }
    let right = drawn
        .iter()
        .map(|(_, rect)| rect.x)
        .min()
        .map_or(area.right().saturating_sub(1), |x| x.saturating_sub(2));
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
    let (message, _) = super::kit::measure::fit(&message, room);
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
