//! One row of buttons. A dialog, a toolbar, and an inline row differ only in their
//! options and in the state of each button.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::UnicodeWidthStr;

use super::super::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    /// The button of a dialog with the focus.
    Focused,
    /// A button of a dialog without the focus, or an inline button.
    Idle,
    /// A button of a toolbar that has an effect now.
    Enabled,
    /// A button of a toolbar that has no effect now.
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowOptions {
    pub align: Align,
    pub gap: u16,
    pub leading_space: bool,
    /// `Some` paints the gaps as the dialog rows do; `None` leaves them untouched.
    pub gap_style: Option<Style>,
    /// A toolbar stops before a button that does not fit. A dialog lets the paragraph
    /// clip the row, as before.
    pub stop_at_edge: bool,
}

impl RowOptions {
    /// The buttons of a dialog: no leading space, and the gaps are painted.
    pub const DIALOG: Self = Self {
        align: Align::Left,
        gap: 2,
        leading_space: false,
        gap_style: Some(Style::new()),
        stop_at_edge: false,
    };

    /// The buttons of a toolbar: one leading space, and the row stops at the edge.
    pub const TOOLBAR: Self = Self {
        align: Align::Left,
        gap: 2,
        leading_space: true,
        gap_style: Some(Style::new()),
        stop_at_edge: true,
    };

    /// The buttons at the right end of a row, over its own background.
    pub const INLINE_RIGHT: Self = Self {
        align: Align::Right,
        gap: 2,
        leading_space: false,
        gap_style: None,
        stop_at_edge: true,
    };

    /// The buttons of the editor review: no painted gaps.
    pub const INLINE_LEFT: Self = Self {
        align: Align::Left,
        gap: 1,
        leading_space: false,
        gap_style: None,
        stop_at_edge: true,
    };
}

/// A button of a row: its identity, its text, and its state.
pub type Item<Id> = (Id, String, ButtonState);

/// Builds the items of a dialog row. `focus` gets the focused style.
pub fn dialog_items<Id: Copy + PartialEq>(
    buttons: impl IntoIterator<Item = (Id, &'static str)>,
    focus: Id,
    theme: &Theme,
) -> Vec<Item<Id>> {
    buttons
        .into_iter()
        .map(|(id, label)| {
            let state = if id == focus {
                ButtonState::Focused
            } else {
                ButtonState::Idle
            };
            (id, theme.button(label), state)
        })
        .collect()
}

/// Builds the items of a toolbar from the enabled flag of each button.
pub fn toolbar_items<Id: Copy>(
    items: impl IntoIterator<Item = (Id, String, bool)>,
) -> Vec<Item<Id>> {
    items
        .into_iter()
        .map(|(id, label, enabled)| {
            let state = if enabled {
                ButtonState::Enabled
            } else {
                ButtonState::Disabled
            };
            (id, label, state)
        })
        .collect()
}

pub fn style(state: ButtonState, theme: &Theme) -> Style {
    match state {
        ButtonState::Focused => theme.dialog_focus,
        ButtonState::Idle => theme.dialog_option,
        ButtonState::Enabled => theme.control,
        ButtonState::Disabled => theme.control_off,
    }
}

/// Draws a row of buttons and returns the area of each one that it draws.
pub fn draw<Id: Copy>(
    frame: &mut Frame,
    area: Rect,
    options: RowOptions,
    items: &[Item<Id>],
    theme: &Theme,
) -> Vec<(Id, Rect)> {
    draw_in_buffer(frame.buffer_mut(), area, options, items, theme)
}

/// Draws a row of buttons in a buffer, for the editor review.
pub fn draw_in_buffer<Id: Copy>(
    buffer: &mut Buffer,
    area: Rect,
    options: RowOptions,
    items: &[Item<Id>],
    theme: &Theme,
) -> Vec<(Id, Rect)> {
    if area.width == 0 || area.height == 0 || items.is_empty() {
        return Vec::new();
    }
    match options.align {
        Align::Left => draw_left(buffer, area, options, items, theme),
        Align::Right => draw_right(buffer, area, options, items, theme),
    }
}

fn width_of(label: &str) -> u16 {
    u16::try_from(label.width()).unwrap_or(u16::MAX)
}

fn draw_left<Id: Copy>(
    buffer: &mut Buffer,
    area: Rect,
    options: RowOptions,
    items: &[Item<Id>],
    theme: &Theme,
) -> Vec<(Id, Rect)> {
    let mut drawn = Vec::new();
    let mut x = area.x.saturating_add(u16::from(options.leading_space));
    for (id, label, _state) in items {
        let width = width_of(label);
        if options.stop_at_edge && x.saturating_add(width) > area.right() {
            break;
        }
        drawn.push((*id, Rect::new(x, area.y, width, 1)));
        x = x.saturating_add(width).saturating_add(options.gap);
    }
    match options.gap_style {
        Some(gap_style) => {
            let row = Rect { height: 1, ..area };
            let mut spans = Vec::new();
            if options.leading_space {
                spans.push(Span::raw(" "));
            }
            for (_, label, state) in items.iter().take(drawn.len()) {
                spans.push(Span::styled(label.clone(), style(*state, theme)));
                spans.push(Span::styled(
                    " ".repeat(usize::from(options.gap)),
                    gap_style,
                ));
            }
            Paragraph::new(Line::from(spans)).render(row, buffer);
        }
        None => {
            for ((_, label, state), (_, rect)) in items.iter().zip(drawn.iter()) {
                if rect.x < buffer.area.right() && rect.y < buffer.area.bottom() {
                    buffer.set_stringn(
                        rect.x,
                        rect.y,
                        label,
                        usize::from(rect.width),
                        style(*state, theme),
                    );
                }
            }
        }
    }
    drawn
}

fn draw_right<Id: Copy>(
    buffer: &mut Buffer,
    area: Rect,
    options: RowOptions,
    items: &[Item<Id>],
    theme: &Theme,
) -> Vec<(Id, Rect)> {
    let mut drawn = Vec::new();
    let mut right = area.right();
    for (id, label, _state) in items.iter().rev() {
        let width = width_of(label);
        let x = right.saturating_sub(width).max(area.x);
        let rect = Rect::new(x, area.y, right.saturating_sub(x), 1);
        drawn.push((*id, rect));
        right = x.saturating_sub(options.gap);
    }
    drawn.reverse();
    for ((_, label, state), (_, rect)) in items.iter().zip(drawn.iter()) {
        if rect.x < buffer.area.right() && rect.y < buffer.area.bottom() {
            buffer.set_stringn(
                rect.x,
                rect.y,
                label,
                usize::from(rect.width),
                style(*state, theme),
            );
        }
    }
    drawn
}
