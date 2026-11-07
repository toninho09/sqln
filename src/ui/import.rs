//! The import dialog.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;
use super::widgets::{
    BORDER_ROWS, FOOTER_ROWS, LEFT, RIGHT, SidebarModal, browse_width, centered, choice_line,
    draw_browse_button, draw_buttons, draw_note, draw_row, draw_sidebar_rows, modal_width,
    value_width,
};
use super::{input_spans, last_chars, text};
use crate::export::NullText;
use crate::grid::printable_start;
use crate::import::form::{Field, ImportForm, Section};
use crate::import::{Encoding, ImportMode, Item};

const WIDTH: u16 = 78;
/// The columns of a label. `Rows per batch` needs them, with one space.
const LABEL_WIDTH: u16 = 16;
const FIRST_FIELD_ROW: u16 = 1;
/// The rows of a section: the preview and the list need a page of rows.
const SECTION_ROWS: u16 = 12;
/// The columns of a value in the preview.
const CELL_WIDTH: usize = 16;
/// The spaces after a cell in the preview.
const CELL_GAP: usize = 2;
/// The columns of the name and the type in the list.
const NAME_WIDTH: usize = 22;
const TYPE_WIDTH: usize = 18;

/// The screen areas of the sidebar and the fields. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct ImportLayout {
    /// One area for each section, in the order of `Section::ALL`.
    pub sections: Vec<Rect>,
    pub fields: Vec<(Field, Rect)>,
    /// The button of a path, with its field.
    pub browse: Option<(Field, Rect)>,
    /// The rows of the column list, with the index of each column.
    pub columns: Vec<(usize, Rect)>,
}

fn label(field: Field) -> &'static str {
    match field {
        Field::File => text::FIELD_EXPORT_FILE,
        Field::Format => text::FIELD_IMPORT_FORMAT,
        Field::Encoding => text::FIELD_IMPORT_ENCODING,
        Field::Header => text::FIELD_EXPORT_HEADER,
        Field::Delimiter => text::FIELD_EXPORT_DELIMITER,
        Field::NullText => text::FIELD_EXPORT_NULL,
        Field::Mode => text::FIELD_IMPORT_MODE,
        Field::Batch => text::FIELD_IMPORT_BATCH,
        Field::Log => text::FIELD_IMPORT_LOG,
        Field::ErrorLimit => text::FIELD_IMPORT_LIMIT,
        _ => "",
    }
}

/// Returns the text of a choice.
fn choice(form: &ImportForm, field: Field) -> String {
    match field {
        Field::Format => text::import_format_name(form.format).to_owned(),
        Field::Encoding => match form.encoding {
            Encoding::Utf8 => text::VALUE_IMPORT_UTF8.to_owned(),
            Encoding::Windows1252 => text::VALUE_IMPORT_WINDOWS_1252.to_owned(),
        },
        Field::Header => text::on_off(form.header).to_owned(),
        Field::Delimiter => text::export_delimiter(char::from(form.delimiter.symbol())),
        Field::NullText => match form.null_text {
            NullText::Empty => text::VALUE_EXPORT_NULL_EMPTY.to_owned(),
            NullText::Word => text::VALUE_EXPORT_NULL_WORD.to_owned(),
        },
        Field::Mode => match form.mode {
            ImportMode::AllOrNothing => text::VALUE_IMPORT_ALL.to_owned(),
            ImportMode::ByBatch => text::VALUE_IMPORT_BATCH.to_owned(),
            ImportMode::LogErrors => text::VALUE_IMPORT_LOG.to_owned(),
        },
        _ => String::new(),
    }
}

fn value<'a>(form: &ImportForm, field: Field, width: usize, theme: &Theme) -> Line<'a> {
    let focused = form.focus == field;
    if let Some(input) = form.input(field) {
        let chars: Vec<char> = input.text().chars().collect();
        // The end of a path holds the name of the file.
        if matches!(field, Field::File | Field::Log) && !focused {
            return Line::raw(last_chars(&chars, width));
        }
        return Line::from(input_spans(&chars, input.cursor(), width, focused, theme).0);
    }
    choice_line(&choice(form, field), focused, theme)
}

/// Writes text in exactly `width` columns.
fn cell(value: &str, width: usize) -> String {
    let mut shown = printable_start(value, width);
    let used = shown.width();
    shown.push_str(&" ".repeat(width.saturating_sub(used)));
    shown
}

/// Draws one row of text, with the left margin of the fields.
fn draw_text(frame: &mut Frame, pane: Rect, y: u16, line: Line) {
    let area = Rect {
        x: pane.x + LEFT,
        y,
        width: pane.width.saturating_sub(LEFT + RIGHT),
        height: 1,
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_fields(
    frame: &mut Frame,
    form: &ImportForm,
    pane: Rect,
    layout: &mut ImportLayout,
    theme: &Theme,
) {
    for (y, field) in (pane.y + FIRST_FIELD_ROW..).zip(form.fields()) {
        let row = Rect {
            y,
            height: 1,
            ..pane
        };
        let is_path = field.is_path();
        let button = if is_path { browse_width(theme) } else { 0 };
        let width = value_width(row, LABEL_WIDTH).saturating_sub(usize::from(button));
        let line = value(form, field, width, theme);
        draw_row(
            frame,
            row,
            (label(field), LABEL_WIDTH),
            form.focus == field,
            line,
            theme,
        );
        if is_path {
            let focused = form.focus == field;
            layout.browse = Some((field, draw_browse_button(frame, row, focused, theme)));
        }
        layout.fields.push((field, row));
    }
}

/// Draws the state of the file below the fields of the first section.
fn draw_summary(frame: &mut Frame, form: &ImportForm, pane: Rect, theme: &Theme) {
    let y = pane.y + FIRST_FIELD_ROW + 3;
    let summary = text::import_summary(
        form.source_columns().len(),
        form.mapped().len(),
        form.table().columns.len(),
    );
    let line = if form.is_reading() {
        text::IMPORT_READING_LINE.to_owned()
    } else if form.probe().is_some() {
        summary
    } else {
        text::IMPORT_CHOOSE_FILE.to_owned()
    };
    draw_text(frame, pane, y, Line::styled(line, theme.hint));
}

/// Keeps the selected row of a list on screen.
fn first_visible(selected: usize, rows: usize) -> usize {
    (selected + 1).saturating_sub(rows.max(1))
}

fn draw_columns(
    frame: &mut Frame,
    form: &ImportForm,
    pane: Rect,
    rows: u16,
    layout: &mut ImportLayout,
    theme: &Theme,
) {
    let width = usize::from(pane.width.saturating_sub(LEFT + RIGHT));
    let source_width = width.saturating_sub(NAME_WIDTH + TYPE_WIDTH + 2);
    let head = format!(
        "{}{}{}",
        cell(text::IMPORT_HEAD_COLUMN, NAME_WIDTH),
        cell(text::IMPORT_HEAD_TYPE, TYPE_WIDTH),
        text::IMPORT_HEAD_SOURCE
    );
    draw_text(
        frame,
        pane,
        pane.y + FIRST_FIELD_ROW,
        Line::styled(head, theme.hint),
    );
    let visible = usize::from(rows.saturating_sub(1));
    let first = first_visible(form.column, visible);
    let focused = form.focus == Field::Columns;
    layout.fields.push((
        Field::Columns,
        Rect {
            height: rows,
            ..pane
        },
    ));
    let list = form.table().columns.iter().enumerate();
    let list = list.skip(first).take(visible);
    for (y, (index, column)) in (pane.y + FIRST_FIELD_ROW + 1..).zip(list) {
        let source = form.mapping.get(index).copied().flatten();
        let source = source.and_then(|source| form.source_columns().get(source));
        let source = source.map_or(text::VALUE_IMPORT_IGNORE, String::as_str);
        let row = format!(
            "{}{}{}",
            cell(&column.name, NAME_WIDTH),
            cell(&column.data_type, TYPE_WIDTH),
            cell(&format!("< {source} >"), source_width)
        );
        let style = match (index == form.column, focused) {
            (true, true) => theme.dialog_focus,
            (true, false) => theme.field_focus,
            _ => Style::new(),
        };
        draw_text(frame, pane, y, Line::styled(row, style));
        layout.columns.push((
            index,
            Rect {
                y,
                height: 1,
                ..pane
            },
        ));
    }
}

fn draw_preview(frame: &mut Frame, form: &ImportForm, pane: Rect, rows: u16, theme: &Theme) {
    let width = usize::from(pane.width.saturating_sub(LEFT + RIGHT));
    let mapped = form.mapped();
    let (first_row, first_column) = form.preview;
    let shown = mapped.iter().skip(first_column);
    let shown: Vec<_> = shown
        .take((width / (CELL_WIDTH + CELL_GAP)).max(1))
        .collect();
    let focused = form.focus == Field::Preview;
    let head_style = if focused {
        theme.panel_title_focused
    } else {
        theme.hint
    };
    let head: String = shown
        .iter()
        .map(|(column, _)| cell(&column.name, CELL_WIDTH + CELL_GAP))
        .collect();
    draw_text(
        frame,
        pane,
        pane.y + FIRST_FIELD_ROW,
        Line::styled(head, head_style),
    );
    let visible = usize::from(rows.saturating_sub(1));
    let items = form.rows().iter().skip(first_row).take(visible);
    for (y, item) in (pane.y + FIRST_FIELD_ROW + 1..).zip(items) {
        let line = match item {
            Item::Row(row) => {
                let cells = shown.iter().map(|(_, source)| {
                    let value = row.cells.get(*source).and_then(Option::as_deref);
                    cell(
                        value.unwrap_or(text::VALUE_IMPORT_NULL),
                        CELL_WIDTH + CELL_GAP,
                    )
                });
                Line::raw(cells.collect::<String>())
            }
            Item::Bad(row) => {
                Line::styled(text::import_bad_row(row.line, &row.reason), theme.error)
            }
        };
        draw_text(frame, pane, y, line);
    }
}

fn draw_format_note(frame: &mut Frame, form: &ImportForm, pane: Rect, theme: &Theme) {
    if form.fields().is_empty() {
        let line = Line::styled(text::IMPORT_JSON_FORMAT, theme.hint);
        draw_text(frame, pane, pane.y + FIRST_FIELD_ROW, line);
    }
}

pub fn draw_import(frame: &mut Frame, form: &ImportForm, theme: &Theme) -> ImportLayout {
    let height = BORDER_ROWS + FIRST_FIELD_ROW + SECTION_ROWS + FOOTER_ROWS;
    let width = modal_width(frame.area().width, WIDTH);
    let area = centered(frame.area(), width, height);
    let SidebarModal { sidebar, pane, .. } =
        SidebarModal::open(frame, area, text::TITLE_IMPORT, theme);

    let names = Section::ALL.map(text::import_section_name);
    let open = Section::ALL
        .iter()
        .position(|section| *section == form.section)
        .unwrap_or(0);
    let mut layout = ImportLayout {
        sections: draw_sidebar_rows(
            frame,
            sidebar,
            &names,
            (open, form.focus == Field::Sections),
            theme,
        ),
        ..ImportLayout::default()
    };

    let title = text::import_title(&form.table().name);
    draw_text(frame, pane, pane.y, Line::styled(title, theme.hint));

    let footer = pane.height.saturating_sub(FOOTER_ROWS + FIRST_FIELD_ROW);
    match form.section {
        Section::File => {
            draw_fields(frame, form, pane, &mut layout, theme);
            draw_summary(frame, form, pane, theme);
        }
        Section::Format => {
            draw_fields(frame, form, pane, &mut layout, theme);
            draw_format_note(frame, form, pane, theme);
        }
        Section::Options => draw_fields(frame, form, pane, &mut layout, theme),
        Section::Columns => draw_columns(frame, form, pane, footer, &mut layout, theme),
        Section::Preview => {
            layout.fields.push((
                Field::Preview,
                Rect {
                    height: footer,
                    ..pane
                },
            ));
            draw_preview(frame, form, pane, footer, theme);
        }
    }

    let content = Rect {
        x: pane.x + LEFT,
        y: pane.y,
        width: pane.width.saturating_sub(LEFT + RIGHT),
        height: 1,
    };
    let button_row = pane.bottom().saturating_sub(2);
    let failure = form.probe().and_then(|probe| probe.failure.as_deref());
    if let Some(note) = form.note.as_deref().or(failure) {
        draw_note(frame, content, button_row, note, theme.error);
    }
    let buttons = [
        (Field::Import, text::BUTTON_IMPORT),
        (Field::Cancel, text::OPTION_CANCEL),
    ];
    layout.fields.extend(draw_buttons(
        frame,
        content,
        button_row,
        (&buttons, form.focus),
        theme,
    ));
    layout
}
