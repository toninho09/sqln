//! The window of the file browser.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::kit::geometry::{MODAL_FREE_ROWS, centered, fitted, modal_width};
use super::kit::list::{draw_prompt, draw_rows, viewport};
use super::kit::measure::last_chars;
use super::kit::modal::open_modal;
use super::kit::style::{self, ListRow};
use super::settings::WIDTH as MODAL_WIDTH;
use super::text;
use super::theme::Theme;
use crate::browser::{FileBrowser, RowView};
use crate::grid::printable;

/// The columns of the modal below that stay visible, so the two borders stay apart.
const MODAL_INSET: u16 = 8;
/// The rows of the list on a small screen.
const MIN_ROWS: u16 = 14;
/// The rows of the list on a tall screen.
const MAX_ROWS: u16 = 35;
/// The rows of the window without the list: the borders, the folder, the filter, and
/// the note.
const CHROME_ROWS: u16 = 5;
/// The rows above the list, inside the borders.
const HEADER_ROWS: u16 = 2;

fn row_text(row: RowView) -> String {
    match row {
        RowView::UseFolder => format!(" {}", text::BROWSER_USE_FOLDER),
        RowView::Parent => " ../".to_owned(),
        RowView::Folder(name) => format!(" {}/", printable(name)),
        RowView::File(name) => format!(" {}", printable(name)),
    }
}

/// Draws the window. It returns the area of the list and the index of its first row.
pub fn draw_browser(frame: &mut Frame, browser: &FileBrowser, theme: &Theme) -> (Rect, usize) {
    let screen = frame.area();
    let width = modal_width(screen.width, MODAL_WIDTH) - MODAL_INSET;
    let height = fitted(
        screen.height,
        MODAL_FREE_ROWS,
        MIN_ROWS + CHROME_ROWS,
        MAX_ROWS + CHROME_ROWS,
    );
    let area = centered(screen, width, height);
    let inner = open_modal(frame, area, text::TITLE_BROWSER, theme);

    // The end of a path holds the open folder.
    let folder = printable(&browser.folder().display().to_string());
    let chars: Vec<char> = folder.chars().collect();
    let width = usize::from(inner.width).saturating_sub(2);
    frame.render_widget(
        Paragraph::new(format!(" {}", last_chars(&chars, width))).style(theme.panel_title),
        Rect { height: 1, ..inner },
    );
    draw_prompt(
        frame,
        browser.filter(),
        Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        },
        theme,
    );

    let list = Rect {
        y: inner.y + HEADER_ROWS,
        height: inner.height.saturating_sub(HEADER_ROWS + 1),
        ..inner
    };
    if let Some(note) = browser.note() {
        let place = Rect {
            y: list.bottom(),
            height: 1,
            ..inner
        };
        let note = Line::styled(format!(" {}", printable(note)), theme.error);
        frame.render_widget(Paragraph::new(note), place);
    }
    if browser.row_count() == 0 {
        let empty = Line::styled(format!(" {}", text::BROWSER_EMPTY), theme.hint);
        frame.render_widget(Paragraph::new(empty), list);
        return (list, 0);
    }
    let rows: Vec<RowView> = browser.rows().collect();
    let view = viewport(browser.cursor(), rows.len(), list.height);
    draw_rows(frame, list, &rows, view, |frame, place, row, selected| {
        let style = if selected {
            style::list_row_style(ListRow::Selected, theme)
        } else if *row == RowView::UseFolder {
            theme.key
        } else {
            style::list_row_style(ListRow::Normal, theme)
        };
        frame.buffer_mut().set_style(place, style);
        frame.render_widget(Paragraph::new(row_text(*row)), place);
    });
    (list, view.first)
}
