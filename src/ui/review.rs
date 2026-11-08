//! The inline review in the editor: ghost text with its buttons, and an inline diff
//! with the review bar.

use edtui::Index2;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::kit::buttons::{self, ButtonState, RowOptions};
use super::theme::Theme;
use crate::editor::review::{Control, Hunk, Keys, Review, Row};
use crate::editor::{SqlEditor, TAB_WIDTH};
use crate::grid::printable_char;

/// Draws the review in the text area of the editor. A ghost shows its buttons after
/// the ghost text; a diff keeps the bar on its last row.
pub fn draw(buffer: &mut Buffer, area: Rect, editor: &mut SqlEditor, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if editor.review().is_some() {
        draw_diff(buffer, area, editor, theme);
    } else if editor.has_ghosts() {
        draw_ghosts(buffer, area, editor, theme);
    }
}

/// Draws an inline diff: the changed lines with the bar of the review on its last row.
fn draw_diff(buffer: &mut Buffer, area: Rect, editor: &mut SqlEditor, theme: &Theme) {
    let lines = editor.state.lines.len();
    let digits = lines.max(1).to_string().len();
    let gutter = (digits + 4).min(usize::from(area.width)) as u16;
    let height = usize::from(area.height.saturating_sub(1)).max(1);
    let text_x = area.x + gutter;
    let width = usize::from(area.width - gutter);
    editor.set_review_height(height);
    let Some(review) = editor.review() else {
        return;
    };
    let keys = &review.keys.clone();
    let total = review.total_rows(lines);
    let top = review.top.min(total.saturating_sub(1));
    let rows = review.rows(lines, top, height);
    // A scroll to the right stops when the longest visible row ends in the view.
    let longest = rows
        .iter()
        .map(|row| match *row {
            Row::Text(line) | Row::Removed { line, .. } => editor.review_row_length(line),
            Row::Added { hunk, index } => review.hunks()[hunk].new[index].chars().count(),
            _ => 0,
        })
        .max()
        .unwrap_or(0);
    let left = review.left.min(longest.saturating_sub(width / 2));
    if let Some(review) = editor.review_mut() {
        review.left = left;
        review.top = top;
    }
    let mut targets = Vec::new();
    buffer.set_style(area, theme.base);
    for (offset, row) in rows.iter().enumerate() {
        let y = area.y + offset as u16;
        let row_area = Rect::new(text_x, y, width as u16, 1);
        let (number, mark) = gutter_of(*row);
        if let Some(number) = number {
            buffer.set_stringn(
                area.x + 1,
                y,
                format!("{number:>digits$}"),
                digits,
                theme.gutter,
            );
        }
        if let Some((mark, style)) = mark_of(mark, theme) {
            buffer.set_stringn(area.x + 2 + digits as u16, y, mark, 1, style);
        }
        let cells = row_cells(editor, *row, left, width, theme);
        put(buffer, row_area, &cells);
        match *row {
            Row::Removed { hunk, .. } | Row::Added { hunk, .. } => {
                targets.push((Control::Hunk(hunk), Rect::new(area.x, y, area.width, 1)));
            }
            Row::Controls(hunk) => {
                buttons_row(
                    buffer,
                    &mut targets,
                    (text_x, y, area.right()),
                    &[
                        (
                            Control::Decide { hunk, accept: true },
                            keys.accept.as_str(),
                            "Accept",
                        ),
                        (
                            Control::Decide {
                                hunk,
                                accept: false,
                            },
                            keys.reject.as_str(),
                            "Reject",
                        ),
                    ],
                    theme,
                );
            }
            _ => {}
        }
    }
    let bar = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    if area.height > 1 || rows.is_empty() {
        draw_bar(
            buffer,
            bar,
            editor.review().expect("review"),
            keys,
            &mut targets,
            theme,
        );
    }
    if let Some(review) = editor.review_mut() {
        review.targets = targets;
    }
}

/// Draws the editor with its ghost answers: each answer replaces its anchor line and
/// ends with a row of buttons. The editor stays editable around them.
fn draw_ghosts(buffer: &mut Buffer, area: Rect, editor: &mut SqlEditor, theme: &Theme) {
    let lines = editor.state.lines.len();
    let digits = lines.max(1).to_string().len();
    let gutter = (digits + 4).min(usize::from(area.width)) as u16;
    let height = usize::from(area.height).max(1);
    let text_x = area.x + gutter;
    let width = usize::from(area.width - gutter);
    editor.set_review_height(height);
    editor.ghost_follow(width);
    let Some(view) = editor.ghost_view() else {
        return;
    };
    let total = editor.ghost_total_rows();
    let top = view.top.min(total.saturating_sub(1));
    let rows = editor.ghost_rows(top, height);
    // A scroll to the right stops when the longest visible row ends in the view.
    let longest = rows
        .iter()
        .map(|row| match *row {
            Row::Text(line) => editor.review_row_length(line),
            Row::Ghost { ghost, line } => ghost_row_length(editor, ghost, line),
            _ => 0,
        })
        .max()
        .unwrap_or(0);
    let left = view.left.min(longest.saturating_sub(width / 2));
    editor.set_ghost_view(top, left);
    let keys = editor.ghost_keys().clone();
    let mut targets = Vec::new();
    buffer.set_style(area, theme.base);
    let cursor = editor.cursor();
    let pendings = editor.ai_pending().to_vec();
    for (offset, row) in rows.iter().enumerate() {
        let y = area.y + offset as u16;
        let row_area = Rect::new(text_x, y, width as u16, 1);
        let (number, mark) = ghost_gutter_of(*row, editor);
        if let Some(number) = number {
            buffer.set_stringn(
                area.x + 1,
                y,
                format!("{number:>digits$}"),
                digits,
                theme.gutter,
            );
        }
        if let Some((mark, style)) = mark_of(mark, theme) {
            buffer.set_stringn(area.x + 2 + digits as u16, y, mark, 1, style);
        }
        // The anchor of a running request shows its animated mark in the first column.
        if let Row::Text(line) = *row
            && let Some((_, since)) = pendings.iter().find(|(at, _)| at.row == line)
            && let Some(cell) = buffer.cell_mut((area.x, y))
        {
            let style = cell.style();
            cell.set_symbol(super::spin_char(*since))
                .set_style(style.patch(theme.hint));
        }
        let cells = ghost_row_cells(editor, *row, left, width, theme);
        put(buffer, row_area, &cells);
        // The cursor of the editor stays visible between the ghosts.
        if let Some(column) = ghost_cursor_column(editor, *row, cursor, left).filter(|c| *c < width)
            && let Some(cell) = buffer.cell_mut((text_x + column as u16, y))
        {
            cell.set_style(theme.cursor);
        }
        if let Row::Ghost {
            ghost: index,
            line: 0,
        } = *row
        {
            let ghost = &editor.ghosts()[index];
            let column = editor_cells(editor, ghost.at.row, left, ghost.at.col);
            if column < width
                && let Some(cell) = buffer.cell_mut((text_x + column as u16, y))
            {
                cell.set_style(theme.cursor);
            }
        }
        if let Row::GhostControls(ghost) = *row {
            buttons_row(
                buffer,
                &mut targets,
                (text_x, y, area.right()),
                &[
                    (
                        Control::GhostDecide {
                            ghost,
                            accept: true,
                        },
                        keys.accept.as_str(),
                        "Accept",
                    ),
                    (
                        Control::GhostDecide {
                            ghost,
                            accept: false,
                        },
                        keys.close.as_str(),
                        keys.close_label,
                    ),
                ],
                theme,
            );
        }
    }
    editor.set_ghost_targets(targets);
}

/// Returns the count of characters of a ghost row, for the stop of the right scroll.
fn ghost_row_length(editor: &SqlEditor, ghost: usize, line: usize) -> usize {
    let Some(ghost) = editor.ghosts().get(ghost) else {
        return 0;
    };
    let Some(answer) = ghost.lines.get(line) else {
        return 0;
    };
    let source = editor.review_row_length(ghost.at.row);
    if line == 0 {
        ghost.at.col + answer.chars().count()
    } else if line + 1 == ghost.lines.len() {
        source + answer.chars().count()
    } else {
        answer.chars().count()
    }
}

/// Returns the line number and the mark of the gutter of a ghost row.
fn ghost_gutter_of(row: Row, editor: &SqlEditor) -> (Option<usize>, char) {
    match row {
        Row::Text(line) => (Some(line + 1), ' '),
        Row::Ghost { ghost, line } => match (line, editor.ghosts().get(ghost)) {
            (0, Some(ghost)) => (Some(ghost.at.row + 1), ' '),
            _ => (None, '+'),
        },
        Row::GhostControls(_) => (None, ' '),
        _ => (None, ' '),
    }
}

/// Returns the line number and the mark of the gutter of a diff row.
fn gutter_of(row: Row) -> (Option<usize>, char) {
    match row {
        Row::Text(line) => (Some(line + 1), ' '),
        Row::Removed { line, .. } => (Some(line + 1), '-'),
        Row::Added { .. } => (None, '+'),
        Row::Controls(_) => (None, ' '),
        _ => (None, ' '),
    }
}

/// Returns true when a character of a line is inside the selection.
fn in_selection(selection: Option<(Index2, Index2)>, line: usize, column: usize) -> bool {
    let Some((start, end)) = selection else {
        return false;
    };
    if line < start.row || line > end.row {
        return false;
    }
    let first = if line == start.row { start.col } else { 0 };
    let last = if line == end.row { end.col } else { usize::MAX };
    column >= first && column < last
}

/// Returns the cell column of the cursor in a row of the ghost view.
fn ghost_cursor_column(editor: &SqlEditor, row: Row, cursor: Index2, left: usize) -> Option<usize> {
    match row {
        Row::Text(line) if line == cursor.row => Some(editor_cells(editor, line, left, cursor.col)),
        Row::Ghost { ghost: index, line } => {
            let ghost = editor.ghosts().get(index)?;
            if cursor.row != ghost.at.row {
                return None;
            }
            if cursor.col <= ghost.at.col {
                return (line == 0).then(|| editor_cells(editor, ghost.at.row, left, cursor.col));
            }
            if line + 1 != ghost.lines.len() {
                return None;
            }
            let prefix: usize = ghost.lines[line].chars().map(cell_width).sum();
            Some(prefix + editor_cells(editor, ghost.at.row, left, cursor.col))
        }
        _ => None,
    }
}

fn mark_of(mark: char, theme: &Theme) -> Option<(&'static str, Style)> {
    match mark {
        '-' => Some(("-", theme.mark_bad)),
        '+' => Some(("+", theme.mark_good)),
        _ => None,
    }
}

fn emphasis(style: Style) -> Style {
    style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

/// Returns the characters of a row of a diff from column `left`, with their styles.
fn row_cells(
    editor: &mut SqlEditor,
    row: Row,
    left: usize,
    width: usize,
    theme: &Theme,
) -> Vec<(char, Style)> {
    let line_cells = |editor: &mut SqlEditor, line: usize, base: Option<Style>| {
        let chars: Vec<char> = editor.review_row_chars(line, left, width);
        let styles = editor.review_row_styles(line, left, chars.len(), theme);
        chars
            .into_iter()
            .zip(styles)
            .map(|(c, style)| (c, base.or(style).unwrap_or(theme.base)))
            .collect::<Vec<_>>()
    };
    let marked = |chars: Vec<(char, Style)>, marks: Option<&Vec<(usize, usize)>>| {
        let Some(marks) = marks else {
            return chars;
        };
        chars
            .into_iter()
            .enumerate()
            .map(|(i, (c, style))| {
                let column = left + i;
                let inside = marks.iter().any(|(a, b)| (*a..*b).contains(&column));
                (c, if inside { emphasis(style) } else { style })
            })
            .collect()
    };
    let review = editor.review().expect("review");
    match row {
        Row::Text(line) => line_cells(editor, line, None),
        Row::Removed { line, hunk } => {
            let marks = review.hunks()[hunk]
                .old_marks
                .get(line - review.hunks()[hunk].line)
                .cloned();
            let cells = line_cells(editor, line, Some(theme.mark_bad));
            marked(cells, marks.as_ref())
        }
        Row::Added { hunk, index } => {
            let hunk: &Hunk = &review.hunks()[hunk];
            let cells = hunk.new[index]
                .chars()
                .skip(left)
                .take(width)
                .map(|c| (c, theme.mark_good))
                .collect();
            marked(cells, hunk.new_marks.get(index))
        }
        _ => Vec::new(),
    }
}

/// Returns the characters of a row of the ghost view from column `left`: the text of
/// the editor, or a line of an answer with the text around its place. The selection
/// of the editor shows, because the editor stays editable.
fn ghost_row_cells(
    editor: &mut SqlEditor,
    row: Row,
    left: usize,
    width: usize,
    theme: &Theme,
) -> Vec<(char, Style)> {
    let selection = editor.selection_range();
    let line_cells = |editor: &mut SqlEditor, line: usize| {
        let chars: Vec<char> = editor.review_row_chars(line, left, width);
        let styles = editor.review_row_styles(line, left, chars.len(), theme);
        chars
            .into_iter()
            .zip(styles)
            .enumerate()
            .map(|(i, (c, style))| {
                let column = left + i;
                let style = if in_selection(selection, line, column) {
                    theme.selection
                } else {
                    style.unwrap_or(theme.base)
                };
                (c, style)
            })
            .collect::<Vec<_>>()
    };
    match row {
        Row::Text(line) => line_cells(editor, line),
        Row::Ghost { ghost: index, line } => {
            let Some(ghost) = editor.ghosts().get(index).cloned() else {
                return Vec::new();
            };
            let Some(answer) = ghost.lines.get(line) else {
                return Vec::new();
            };
            let ghost_style = theme.hint.add_modifier(Modifier::ITALIC);
            let source: Vec<char> = editor.review_row_chars(ghost.at.row, 0, usize::MAX);
            let styles = editor.review_row_styles(ghost.at.row, 0, source.len(), theme);
            let own = |range: std::ops::Range<usize>| {
                range
                    .map(|i| {
                        let style = if in_selection(selection, ghost.at.row, i) {
                            theme.selection
                        } else {
                            styles.get(i).copied().flatten().unwrap_or(theme.base)
                        };
                        (source[i], style)
                    })
                    .collect::<Vec<_>>()
            };
            let mut cells = Vec::new();
            if line == 0 {
                cells.extend(own(0..ghost.at.col.min(source.len())));
            }
            cells.extend(answer.chars().map(|c| (c, ghost_style)));
            if line + 1 == ghost.lines.len() {
                cells.extend(own(ghost.at.col.min(source.len())..source.len()));
            }
            cells.into_iter().skip(left).take(width).collect()
        }
        _ => Vec::new(),
    }
}

/// Returns the screen cells of the characters of a line from `left` to `column`.
fn editor_cells(editor: &SqlEditor, line: usize, left: usize, column: usize) -> usize {
    editor
        .review_row_chars(line, left, column.saturating_sub(left))
        .into_iter()
        .map(cell_width)
        .sum()
}

fn cell_width(c: char) -> usize {
    match c {
        '\t' => TAB_WIDTH,
        c => printable_char(c).width().unwrap_or(0),
    }
}

/// Writes characters in one row. A tab fills `TAB_WIDTH` cells, and a control
/// character shows as a visible mark.
fn put(buffer: &mut Buffer, area: Rect, cells: &[(char, Style)]) {
    let mut x = area.x;
    for (c, style) in cells {
        let width = cell_width(*c) as u16;
        if x + width > area.right() {
            break;
        }
        let symbol = if *c == '\t' {
            " ".repeat(TAB_WIDTH)
        } else {
            printable_char(*c).to_string()
        };
        buffer.set_stringn(x, area.y, symbol, usize::from(width), *style);
        x += width;
    }
}

/// Writes a button and keeps its place. It returns the column after the button.
fn button(
    buffer: &mut Buffer,
    targets: &mut Vec<(Control, Rect)>,
    (x, y, right): (u16, u16, u16),
    control: Control,
    key: &str,
    label: &str,
    theme: &Theme,
) -> u16 {
    let text = if label.is_empty() {
        theme.button(key)
    } else if key.is_empty() {
        theme.button(label)
    } else {
        theme.button(&format!("{key} {label}"))
    };
    let width = text.width() as u16;
    let area = Rect::new(x, y, right.saturating_sub(x), 1);
    let items = [(control, text, ButtonState::Idle)];
    match buttons::draw_in_buffer(buffer, area, RowOptions::INLINE_LEFT, &items, theme).first() {
        Some((_, rect)) => {
            targets.push((control, *rect));
            x.saturating_add(width).saturating_add(1)
        }
        None => right,
    }
}

/// Draws a row of buttons at `(x, y)`. When the labels do not fit, each button shows
/// only its key.
fn buttons_row(
    buffer: &mut Buffer,
    targets: &mut Vec<(Control, Rect)>,
    (x, y, right): (u16, u16, u16),
    buttons: &[(Control, &str, &str)],
    theme: &Theme,
) {
    let room = usize::from(right.saturating_sub(x));
    let size = |full: bool| {
        buttons
            .iter()
            .map(|(_, key, label)| {
                let text = if key.is_empty() || !full {
                    (*label).to_owned()
                } else {
                    format!("{key} {label}")
                };
                theme.button(&text).width() + 1
            })
            .sum::<usize>()
    };
    let full = size(true) <= room;
    let mut x = x;
    for (control, key, label) in buttons {
        let label = if full || key.is_empty() { *label } else { "" };
        x = button(buffer, targets, (x, y, right), *control, key, label, theme);
    }
}

/// The columns of the title that a narrow bar keeps, such as ` 1/2 changes`.
const MIN_TITLE: usize = 13;

/// Draws the bar of a diff: the origin, the place of the current change, and the
/// buttons for all changes. A narrow bar shows only the keys, then drops the text
/// before the buttons.
fn draw_bar(
    buffer: &mut Buffer,
    area: Rect,
    review: &Review,
    keys: &Keys,
    targets: &mut Vec<(Control, Rect)>,
    theme: &Theme,
) {
    buffer.set_style(area, theme.bar.add_modifier(Modifier::REVERSED));
    // The place goes first, so a narrow bar keeps it.
    let (place, count) = review.place();
    let noun = if count == 1 { "change" } else { "changes" };
    let mut title = format!(" {place}/{count} {noun} · {}", review.label);
    if !review.note.is_empty() {
        title.push_str(&format!(" · {}", review.note));
    }
    title.push(' ');
    let title = crate::grid::printable(&title);
    // The buttons of one change sit at the change; the bar decides all changes.
    let buttons: Vec<(Control, &str, &str)> = vec![
        (Control::AcceptAll, keys.accept_all.as_str(), "Accept all"),
        (Control::RejectAll, keys.reject_all.as_str(), "Reject all"),
        (Control::Close, keys.close.as_str(), keys.close_label),
    ];
    // The first `labels` buttons show their label. The others show only their key.
    let size = |labels: usize| -> usize {
        buttons
            .iter()
            .enumerate()
            .map(|(index, (_, key, label))| {
                let text = if key.is_empty() {
                    (*label).to_owned()
                } else if index < labels {
                    format!("{key} {label}")
                } else {
                    (*key).to_owned()
                };
                theme.button(&text).width() + 1
            })
            .sum()
    };
    let room = usize::from(area.width);
    let count = buttons.len();
    // The title keeps at least its first part, then the labels get the room.
    let least = title.width().min(MIN_TITLE);
    let fits = |title: usize| {
        (0..=count)
            .rev()
            .find(|labels| title + size(*labels) <= room)
    };
    let (shown_title, labels) = match fits(least) {
        Some(labels) => {
            let free = room - size(labels);
            (super::kit::measure::fit(&title, free).0, labels)
        }
        None => (String::new(), fits(0).unwrap_or(0)),
    };
    buffer.set_stringn(
        area.x,
        area.y,
        &shown_title,
        room,
        theme.bar.add_modifier(Modifier::REVERSED),
    );
    let mut x = area.x + shown_title.width() as u16;
    for (index, (control, key, label)) in buttons.into_iter().enumerate() {
        let label = if index < labels || key.is_empty() {
            label
        } else {
            ""
        };
        x = button(
            buffer,
            targets,
            (x, area.y, area.right()),
            control,
            key,
            label,
            theme,
        );
    }
}
