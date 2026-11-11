//! Screen drawing.

pub mod agent;
pub mod ai;
pub mod auth;
pub mod browser;
pub mod compare;
pub mod conflicts;
pub mod console;
pub mod data_edit;
pub mod diagram;
pub mod export;
pub mod form;
pub mod help;
pub mod history;
pub mod icons;
pub mod import;
pub mod kit;
pub mod messages;
pub mod name;
pub mod navigator;
pub mod palette;
pub mod parameters;
pub mod review;
pub mod schemes;
pub mod searchbar;
pub mod settings;
pub mod statusline;
pub mod table;
pub mod text;
pub mod theme;
pub mod welcome;
pub mod which_key;
pub mod wrap;

use edtui::{EditorTheme, EditorView, LineNumbers};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget,
};

pub use self::conflicts::{ConflictLayout, ConflictRowView, draw_conflicts};
pub use self::console::{
    ConsoleLayout, MainTab, TabView, column_edge_at, draw_console, draw_dialog, draw_main_tabs,
    draw_overlay, draw_picker, draw_small_terminal, grid_cell_at, grid_row_number_at,
    header_column_at, main_tab_label, run_badge, tab_at,
};
use self::kit::list::viewport;
use self::kit::style::{self, ListRow};
pub use self::table::{TableBar, TableControl, TableLayout, draw_table, section_at};
use self::theme::Theme;
use crate::complete::{Suggestion, SuggestionKind};
use crate::editor::{MarkState, SearchBar, SearchField, SqlEditor, SuggestionList, TAB_WIDTH};

/// The columns of the gutter, without the digits: a space before the number, a space
/// after it, the state mark, and a space before the text.
const GUTTER_COLUMNS: u16 = 4;
const MAX_LIST_ROWS: usize = 8;
const MIN_LIST_WIDTH: u16 = 24;

/// Draws a scrollbar on the last column of an area, when the content needs one.
pub(crate) fn draw_scrollbar(
    buffer: &mut Buffer,
    area: Rect,
    total: usize,
    visible: usize,
    position: usize,
    theme: &Theme,
) {
    if total <= visible || area.height == 0 || area.width == 0 {
        return;
    }
    let mut state = ScrollbarState::new(total - visible)
        .position(position)
        .viewport_content_length(visible);
    Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .thumb_symbol("▐")
        .track_style(theme.scrollbar_track)
        .thumb_style(theme.scrollbar_thumb)
        .render(area, buffer, &mut state);
}

pub fn draw_editor(
    frame: &mut Frame,
    area: Rect,
    editor: &mut SqlEditor,
    focused: bool,
    theme: &Theme,
) {
    let warning = editor
        .syntax_report()
        .and_then(|report| text::syntax_summary(report, editor.cursor().row));
    let bar_height = if editor.search().is_some() { 2 } else { 0 };
    let [bar, text, warning_area] = Layout::vertical([
        Constraint::Length(bar_height),
        Constraint::Min(1),
        Constraint::Length(u16::from(warning.is_some() && area.height > bar_height + 1)),
    ])
    .areas(area);
    if let Some(warning) = warning {
        let warning = crate::grid::printable(&warning);
        let warning = kit::measure::fit(&warning, warning_area.width.into()).0;
        frame.render_widget(Paragraph::new(warning).style(theme.error), warning_area);
    }

    if let Some(search) = editor.search() {
        draw_search_bar(frame, bar, search);
    }

    // The last column holds the scrollbar, so the text keeps its width in a long script.
    let [text, scroll] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(1)]).areas(text);
    // The application draws the line numbers, because EdTUI leaves no space around them.
    if editor.review().is_some() || editor.has_ghosts() {
        editor.set_text_area(gutterless(text, editor));
        review::draw(frame.buffer_mut(), text, editor, theme);
        return;
    }
    let text_area = gutterless(text, editor);
    editor.set_text_area(text_area);
    let (width, height) = (text_area.width.into(), text_area.height.into());
    let mut frame_state = editor.prepare_frame(width, height, theme);
    let editor_theme = EditorTheme::default()
        .base(theme.base)
        .cursor_style(theme.cursor)
        .selection_style(theme.selection)
        .hide_status_line();
    EditorView::new(frame_state.state())
        .theme(editor_theme)
        .wrap(false)
        .tab_width(TAB_WIDTH)
        .line_numbers(LineNumbers::None)
        .render(text_area, frame.buffer_mut());
    frame_state.paint(text_area, frame.buffer_mut(), theme.cursor);
    drop(frame_state);

    if focused {
        draw_current_line(frame.buffer_mut(), text, editor, theme);
    }
    draw_gutter(frame.buffer_mut(), text, editor, focused, theme);
    draw_marks(frame, text, editor, theme);
    draw_ai_anchor(frame.buffer_mut(), text, editor, theme);
    let (_, top) = editor.state.viewport_offset();
    draw_scrollbar(
        frame.buffer_mut(),
        scroll,
        editor.state.lines.len(),
        usize::from(text.height),
        top,
        theme,
    );

    let anchor_shift = editor.list_anchor_chars();
    let cursor = editor.state.cursor_screen_position();
    if let (Some(list), Some(cursor)) = (editor.suggestions(), cursor) {
        let anchor = Position::new(cursor.x.saturating_sub(anchor_shift as u16), cursor.y);
        let layout = draw_suggestions(frame, area, anchor, list, theme);
        editor.set_list_area(layout);
    }
}

/// Writes the visible part of a text field. The cursor stays inside `width`.
/// It returns the spans and the index of the first visible character.
pub(crate) fn input_spans(
    chars: &[char],
    cursor: usize,
    width: usize,
    focused: bool,
    theme: &Theme,
) -> (Vec<Span<'static>>, usize) {
    if !focused {
        return (
            vec![Span::raw(chars.iter().take(width).collect::<String>())],
            0,
        );
    }
    let cursor = cursor.min(chars.len());
    let first = (cursor + 1).saturating_sub(width);
    let before: String = chars[first..cursor].iter().collect();
    let at = chars
        .get(cursor)
        .map_or_else(|| " ".to_owned(), char::to_string);
    let after: String = chars
        .iter()
        .skip(cursor + 1)
        .take(width.saturating_sub(cursor + 1 - first))
        .collect();
    (
        vec![
            Span::raw(before),
            Span::styled(at, theme.cursor),
            Span::raw(after),
        ],
        first,
    )
}

fn mark_symbol(state: MarkState, theme: &Theme) -> (&'static str, Style) {
    match state {
        MarkState::Running => ("●", theme.mark_neutral),
        MarkState::Success => ("✓", theme.mark_good),
        MarkState::Capped => ("⋯", theme.mark_good),
        MarkState::Error => ("✗", theme.mark_bad),
        MarkState::NotExecuted => ("—", theme.mark_neutral),
        MarkState::Stopped => ("■", theme.mark_neutral),
        MarkState::Timeout => ("◷", theme.mark_bad),
        MarkState::Unknown => ("?", theme.mark_bad),
    }
}

/// Marks the row of the cursor. A cell with its own background keeps it, so the marks
/// of the selection and the search stay.
fn draw_current_line(buffer: &mut Buffer, area: Rect, editor: &SqlEditor, theme: &Theme) {
    let (_, top) = editor.state.viewport_offset();
    let Some(offset) = editor
        .state
        .cursor
        .row
        .checked_sub(top)
        .filter(|offset| *offset < usize::from(area.height))
    else {
        return;
    };
    let y = area.y + offset as u16;
    let Some(line) = theme.cursor_line.bg else {
        return;
    };
    let base = theme.base.bg.unwrap_or(Color::Reset);
    for x in area.x..area.right() {
        if let Some(cell) = buffer.cell_mut((x, y))
            && cell.bg == base
        {
            cell.set_bg(line);
        }
    }
}

/// Writes the line numbers.
fn draw_gutter(buffer: &mut Buffer, area: Rect, editor: &SqlEditor, focused: bool, theme: &Theme) {
    let digits = usize::from((gutterless(area, editor).x - area.x).saturating_sub(GUTTER_COLUMNS));
    let (_, top) = editor.state.viewport_offset();
    let rows = editor.state.lines.len().max(1);
    for (y, row) in (area.y..area.bottom()).zip(top..rows) {
        let current = focused && row == editor.state.cursor.row;
        let style = if current {
            theme.gutter_current
        } else {
            theme.gutter
        };
        buffer.set_stringn(
            area.x + 1,
            y,
            format!("{:>digits$}", row + 1),
            digits,
            style,
        );
    }
}

// Shows the anchor of a running completion: an animated mark in the first column of
// the gutter of the requested line. The user edits other lines and still sees where
// the answer will land.
fn draw_ai_anchor(buffer: &mut Buffer, area: Rect, editor: &SqlEditor, theme: &Theme) {
    let (_, top) = editor.state.viewport_offset();
    for (at, since) in editor.ai_pending() {
        let Some(offset) = at
            .row
            .checked_sub(top)
            .filter(|offset| *offset < usize::from(area.height))
        else {
            continue;
        };
        if let Some(cell) = buffer.cell_mut((area.x, area.y + offset as u16)) {
            let style = cell.style();
            cell.set_symbol(spin_char(*since))
                .set_style(style.patch(theme.hint));
        }
    }
}

/// Returns the animated mark of a request that runs since `since`.
pub(crate) fn spin_char(since: std::time::Instant) -> &'static str {
    const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
    const FRAME_TIME: u128 = 100;
    let frame =
        usize::try_from(since.elapsed().as_millis() / FRAME_TIME).unwrap_or(0) % FRAMES.len();
    FRAMES[frame]
}

/// Writes the statement states in the free column of the line number gutter.
fn draw_marks(frame: &mut Frame, area: Rect, editor: &mut SqlEditor, theme: &Theme) {
    let (_, top) = editor.state.viewport_offset();
    let x = gutterless(area, editor).x.saturating_sub(2);
    let buffer = frame.buffer_mut();
    for mark in editor.marks() {
        let Some(state) = mark.state else { continue };
        let Some(offset) = mark
            .row
            .checked_sub(top)
            .filter(|offset| *offset < usize::from(area.height))
        else {
            continue;
        };
        let (symbol, style) = mark_symbol(state, theme);
        if let Some(cell) = buffer.cell_mut((x, area.y + offset as u16)) {
            cell.set_symbol(symbol).set_style(style);
        }
    }
}

/// Returns the text columns of the editor area, without the line numbers.
fn gutterless(area: Rect, editor: &SqlEditor) -> Rect {
    let digits = editor.state.lines.len().max(1).to_string().len() as u16;
    let gutter = (digits + GUTTER_COLUMNS).min(area.width);
    Rect {
        x: area.x + gutter,
        width: area.width - gutter,
        ..area
    }
}

fn draw_search_bar(frame: &mut Frame, area: Rect, search: &SearchBar) {
    let [find, replace] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let marker = |field| if search.field == field { "> " } else { "  " };
    let count = if search.matches.is_empty() {
        text::NO_MATCHES.to_owned()
    } else {
        format!("{} of {}", search.current + 1, search.matches.len())
    };
    let find_line = format!(
        "{}{}{}  {count}",
        marker(SearchField::Find),
        text::FIND_LABEL,
        search.find
    );
    let replace_line = format!(
        "{}{}{}",
        marker(SearchField::Replace),
        text::REPLACE_LABEL,
        search.replace
    );
    frame.render_widget(Paragraph::new(find_line), find);
    frame.render_widget(Paragraph::new(replace_line), replace);
}

fn kind_tag(kind: SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Join => "fk ",
        SuggestionKind::Column => "col",
        SuggestionKind::Alias => "als",
        SuggestionKind::Cte => "cte",
        SuggestionKind::Table => "tbl",
        SuggestionKind::View => "vw ",
        SuggestionKind::Schema => "sch",
        SuggestionKind::Function => "fn ",
        SuggestionKind::DataType => "typ",
        SuggestionKind::Keyword => "kw ",
    }
}

/// Places the list below the anchor. Without room, it uses the larger side.
pub fn list_area(anchor: Position, bounds: Rect, width: u16, rows: u16) -> Rect {
    let width = width.min(bounds.width);
    let x = anchor
        .x
        .min(bounds.right().saturating_sub(width))
        .max(bounds.x);
    let below = bounds.bottom().saturating_sub(anchor.y.saturating_add(1));
    let above = anchor.y.saturating_sub(bounds.y);
    if rows <= below || below >= above {
        Rect::new(x, anchor.y.saturating_add(1), width, rows.min(below))
    } else {
        let height = rows.min(above);
        Rect::new(x, anchor.y - height, width, height)
    }
}

/// Draws a list below the anchor, with the marked row in view. `row` gives the spans of a
/// row from its index and its mark. It returns the area of the list and its first row.
pub(crate) fn draw_anchored_list<'a>(
    frame: &mut Frame,
    (bounds, anchor): (Rect, Position),
    (count, selected, width): (usize, Option<usize>, usize),
    theme: &Theme,
    row: impl Fn(usize, bool) -> Vec<Span<'a>>,
) -> (Rect, usize) {
    let rows = count.min(MAX_LIST_ROWS) as u16;
    let width = u16::try_from(width).unwrap_or(u16::MAX).max(MIN_LIST_WIDTH);
    let area = list_area(anchor, bounds, width, rows);
    let view = viewport(selected.unwrap_or(0), count, area.height);
    let first = view.first;
    let visible = view.rows;
    frame.render_widget(Clear, area);
    for (y, index) in (area.y..).zip((first..count).take(visible)) {
        let marked = Some(index) == selected;
        let style = style::list_row_style(
            if marked {
                ListRow::Selected
            } else {
                ListRow::Normal
            },
            theme,
        );
        let line = Line::from(row(index, marked));
        let rect = Rect {
            y,
            height: 1,
            ..area
        };
        frame.render_widget(Paragraph::new(line).style(style), rect);
    }
    (area, first)
}

/// The AI rows above the local items: the request row, then the plain row.
const AI_ROW_LABELS: [&str; crate::editor::AI_ROWS] = [
    " AI  Complete with AI with a request…",
    " AI  Complete with AI…",
];

pub(crate) fn draw_suggestions(
    frame: &mut Frame,
    bounds: Rect,
    anchor: Position,
    list: &SuggestionList,
    theme: &Theme,
) -> (Rect, usize) {
    let items = &list.completion.items;
    let row_width = |item: &Suggestion| {
        let detail = item
            .detail
            .as_deref()
            .map_or(0, |text| Span::raw(text).width() + 2);
        Span::raw(item.label.as_str()).width() + detail + 6
    };
    let ai = crate::editor::ai_row_count(list.ai);
    let mut width = items.iter().map(row_width).max().unwrap_or_default();
    if ai > 0 {
        let row = AI_ROW_LABELS
            .iter()
            .map(|label| Span::raw(*label).width() + 3)
            .max()
            .unwrap_or_default();
        width = width.max(row);
    }
    let shape = (items.len() + ai, Some(list.selected), width);
    draw_anchored_list(frame, (bounds, anchor), shape, theme, |index, marked| {
        if list.ai && index < ai {
            return vec![Span::styled(
                AI_ROW_LABELS[index],
                style::list_row_style(
                    if marked {
                        ListRow::Selected
                    } else {
                        ListRow::Normal
                    },
                    theme,
                ),
            )];
        }
        let item = &items[index - ai];
        let style = style::list_row_style(
            if marked {
                ListRow::Selected
            } else {
                ListRow::Normal
            },
            theme,
        );
        let detail_style = if marked {
            theme.list_selected
        } else {
            theme.list_detail
        };
        let mut spans = vec![
            Span::styled(format!(" {} ", kind_tag(item.kind)), detail_style),
            Span::styled(item.label.as_str(), style),
        ];
        if let Some(detail) = &item.detail {
            spans.push(Span::styled(format!("  {detail}"), detail_style));
        }
        spans
    })
}
