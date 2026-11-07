//! The chrome of a modal window: the border, the title, the sidebar, the content,
//! and the footer.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph, Wrap};

use super::super::theme::Theme;
use super::buttons::{self, Item, RowOptions};
use super::geometry::{BORDER_ROWS, FOOTER_ROWS, LEFT, NOTE_ROWS, RIGHT, SIDEBAR_WIDTH, centered};

/// Returns the block of a window above the panels.
pub fn float_block<'a>(title: &str, theme: &Theme) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.float_border)
        .title(Span::styled(format!(" {title} "), theme.float_title))
        .style(theme.dialog)
}

/// Opens a floating window: it clears the area, draws the border, and returns the
/// place of the contents.
pub fn open_modal(frame: &mut Frame, area: Rect, title: &str, theme: &Theme) -> Rect {
    frame.render_widget(Clear, area);
    let block = float_block(title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// The areas of a modal with a sidebar: the contents, the rows of the sidebar, and the
/// fields over them.
pub struct SidebarModal {
    pub inner: Rect,
    pub sidebar: Rect,
    pub pane: Rect,
}

impl SidebarModal {
    /// Opens a floating window with a sidebar on the left and a divider between the
    /// sides.
    pub fn open(frame: &mut Frame, area: Rect, title: &str, theme: &Theme) -> Self {
        let inner = open_modal(frame, area, title, theme);
        let sidebar = Rect {
            y: inner.y + 1,
            width: SIDEBAR_WIDTH,
            height: inner.height.saturating_sub(1),
            ..inner
        };
        draw_divider(frame, area, sidebar.right(), theme);
        let pane = Rect {
            x: sidebar.right() + 1,
            width: inner.width.saturating_sub(SIDEBAR_WIDTH + 1),
            ..inner
        };
        Self {
            inner,
            sidebar,
            pane,
        }
    }
}

/// Draws the line between the sidebar and the fields. The line joins the window border.
pub fn draw_divider(frame: &mut Frame, window: Rect, x: u16, theme: &Theme) {
    for y in window.y..window.bottom() {
        let symbol = if y == window.y {
            "┬"
        } else if y + 1 == window.bottom() {
            "┴"
        } else {
            "│"
        };
        frame
            .buffer_mut()
            .set_string(x, y, symbol, theme.float_border);
    }
}

/// Draws the rows of a sidebar and returns them. The open row stays marked when the
/// focus goes to the fields, as the row of the explorer does.
pub fn draw_sidebar_rows(
    frame: &mut Frame,
    area: Rect,
    titles: &[&str],
    (open, focused): (usize, bool),
    theme: &Theme,
) -> Vec<Rect> {
    let mut rows = Vec::new();
    for (y, (index, title)) in (area.y..).zip(titles.iter().enumerate()) {
        let row = Rect {
            y,
            height: 1,
            ..area
        };
        rows.push(row);
        if index == open {
            let style = if focused {
                theme.tree_selected
            } else {
                theme.tree_cursor
            };
            frame.buffer_mut().set_style(row, style);
        }
        if index == open && focused {
            let mark = Span::styled(theme.icons.focus_mark, theme.panel_title_focused);
            frame.render_widget(
                Paragraph::new(mark),
                Rect {
                    x: row.x + 1,
                    width: 1,
                    ..row
                },
            );
        }
        frame.render_widget(
            Paragraph::new(*title),
            Rect {
                x: row.x + LEFT,
                width: row.width.saturating_sub(LEFT),
                ..row
            },
        );
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Center,
    Top,
    Bottom,
    BottomRight,
}

pub struct SidebarSpec<'a> {
    pub titles: &'a [&'a str],
    pub open: usize,
    pub focused: bool,
}

pub struct Modal<'a> {
    pub title: String,
    pub width: u16,
    pub height: u16,
    pub placement: Placement,
    pub heading_row: u16,
    pub sidebar: Option<SidebarSpec<'a>>,
}

pub struct ModalLayout {
    pub area: Rect,
    pub inner: Rect,
    pub sidebar: Option<Rect>,
    pub sidebar_rows: Vec<Rect>,
    pub pane: Rect,
    /// The columns of the content, one row high.
    pub content: Rect,
    /// The row of the title inside the pane.
    pub heading: Rect,
    /// The row of the buttons.
    pub button_row: u16,
    /// The rows of the note above the buttons.
    pub note: Rect,
}

impl<'a> Modal<'a> {
    pub fn sized(width: u16, height: u16) -> Self {
        Self {
            title: String::new(),
            width,
            height,
            placement: Placement::Center,
            heading_row: 0,
            sidebar: None,
        }
    }

    /// A form window: the height holds `fields` rows from `first_row`.
    pub fn form(width: u16, first_row: u16, fields: u16) -> Self {
        Self::sized(
            width,
            BORDER_ROWS
                .saturating_add(first_row)
                .saturating_add(fields)
                .saturating_add(FOOTER_ROWS),
        )
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn heading_row(mut self, row: u16) -> Self {
        self.heading_row = row;
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn sidebar(mut self, sidebar: SidebarSpec<'a>) -> Self {
        self.sidebar = Some(sidebar);
        self
    }

    pub fn open(self, frame: &mut Frame, theme: &Theme) -> ModalLayout {
        let area = self.place(frame.area());
        let (inner, sidebar, sidebar_rows, pane) = match self.sidebar {
            Some(spec) => {
                let window = SidebarModal::open(frame, area, &self.title, theme);
                let rows = draw_sidebar_rows(
                    frame,
                    window.sidebar,
                    spec.titles,
                    (spec.open, spec.focused),
                    theme,
                );
                (window.inner, Some(window.sidebar), rows, window.pane)
            }
            None => {
                let inner = open_modal(frame, area, &self.title, theme);
                (inner, None, Vec::new(), inner)
            }
        };
        let content = Rect {
            x: pane.x + LEFT,
            y: pane.y.saturating_add(self.heading_row),
            width: pane.width.saturating_sub(LEFT + RIGHT),
            height: 1,
        };
        let heading = Rect {
            y: pane.y.saturating_add(self.heading_row),
            ..content
        };
        let button_row = pane.bottom().saturating_sub(2);
        let note = Rect {
            y: button_row.saturating_sub(NOTE_ROWS),
            height: NOTE_ROWS,
            ..content
        };
        ModalLayout {
            area,
            inner,
            sidebar,
            sidebar_rows,
            pane,
            content,
            heading,
            button_row,
            note,
        }
    }

    fn place(&self, screen: Rect) -> Rect {
        let width = self.width.min(screen.width);
        let height = self.height.min(screen.height);
        match self.placement {
            Placement::Center => centered(screen, width, height),
            Placement::Top => Rect::new(
                screen.x + screen.width.saturating_sub(width) / 2,
                screen.y,
                width,
                height,
            ),
            Placement::Bottom => Rect::new(
                screen.x + screen.width.saturating_sub(width) / 2,
                screen.bottom().saturating_sub(height),
                width,
                height,
            ),
            Placement::BottomRight => Rect::new(
                screen.right().saturating_sub(width),
                screen.bottom().saturating_sub(height),
                width,
                height,
            ),
        }
    }
}

/// The note and the hint above the buttons of a modal.
pub struct Footer<'a> {
    pub note: Option<(&'a str, Style)>,
    pub hint: Option<Line<'a>>,
}

pub fn draw_footer<Id: Copy>(
    frame: &mut Frame,
    layout: &ModalLayout,
    footer: Footer<'_>,
    buttons: &[Item<Id>],
    theme: &Theme,
) -> Vec<(Id, Rect)> {
    if let Some((note, style)) = footer.note {
        super::feedback::draw_note(frame, layout.content, layout.button_row, note, style);
    }
    if let Some(hint) = footer.hint {
        frame.render_widget(Paragraph::new(hint).wrap(Wrap { trim: false }), layout.note);
    }
    buttons::draw(
        frame,
        Rect {
            y: layout.button_row,
            height: 1,
            ..layout.content
        },
        RowOptions::DIALOG,
        buttons,
        theme,
    )
}

/// A card at the bottom right of an area, with the warning border.
pub fn warning_block<'a>(title: &str, theme: &Theme) -> Block<'a> {
    let warning = Style {
        bg: None,
        ..theme.status_warning
    };
    float_block(title, theme)
        .border_style(warning)
        .title_style(warning)
        .padding(Padding::horizontal(1))
}
