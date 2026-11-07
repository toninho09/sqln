//! The export dialog.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::settings::WIDTH;
use super::theme::Theme;
use super::widgets::{
    BORDER_ROWS, FOOTER_ROWS, SidebarModal, browse_width, centered, choice_line,
    draw_browse_button, draw_buttons, draw_note, draw_row, draw_sidebar_rows, modal_width,
    value_width,
};
use super::widgets::{LEFT, RIGHT};
use super::{input_spans, last_chars, text};
use crate::export::NullText;
use crate::export::form::{ExportForm, Field, RowChoice};

/// The columns of a label. `Rows per INSERT` needs them, with one space.
const LABEL_WIDTH: u16 = 16;
const FIRST_FIELD_ROW: u16 = 1;
/// The fields of the largest format: `Rows`, `File`, and four options of CSV.
const MAX_FIELDS: u16 = 6;

/// The screen areas of the sidebar and the fields. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct ExportLayout {
    /// One area for each format of the form, in the same order.
    pub formats: Vec<Rect>,
    pub fields: Vec<(Field, Rect)>,
    /// The button of the path.
    pub browse: Option<Rect>,
}

fn label(field: Field) -> &'static str {
    match field {
        Field::Rows => text::FIELD_EXPORT_ROWS,
        Field::File => text::FIELD_EXPORT_FILE,
        Field::Header => text::FIELD_EXPORT_HEADER,
        Field::Delimiter => text::FIELD_EXPORT_DELIMITER,
        Field::NullText => text::FIELD_EXPORT_NULL,
        Field::Bom => text::FIELD_EXPORT_BOM,
        Field::Table => text::FIELD_EXPORT_TABLE,
        Field::Grouping => text::FIELD_EXPORT_GROUPING,
        Field::Batch => text::FIELD_EXPORT_BATCH,
        Field::Formats | Field::Export | Field::Cancel => "",
    }
}

/// Returns the text of a choice.
fn choice(form: &ExportForm, field: Field) -> String {
    let source = form.source();
    match field {
        Field::Rows => match (form.rows, source.selection) {
            (RowChoice::Selection, Some((rows, columns))) => text::export_selection(rows, columns),
            (RowChoice::AllRows, _) => text::VALUE_EXPORT_ALL_ROWS.to_owned(),
            _ => text::export_loaded(source.loaded),
        },
        Field::Header => text::on_off(form.header).to_owned(),
        Field::Delimiter => text::export_delimiter(char::from(form.delimiter.symbol())),
        Field::NullText => match form.null_text {
            NullText::Empty => text::VALUE_EXPORT_NULL_EMPTY.to_owned(),
            NullText::Word => text::VALUE_EXPORT_NULL_WORD.to_owned(),
        },
        Field::Bom => text::on_off(form.bom).to_owned(),
        Field::Grouping if form.grouped => text::VALUE_EXPORT_GROUPED.to_owned(),
        Field::Grouping => text::VALUE_EXPORT_ONE_ROW.to_owned(),
        _ => String::new(),
    }
}

fn value<'a>(form: &ExportForm, field: Field, width: usize, theme: &Theme) -> Line<'a> {
    let focused = form.focus == field;
    if let Some(input) = form.input(field) {
        let chars: Vec<char> = input.text().chars().collect();
        // The end of a path holds the name of the file and its extension.
        if field == Field::File && !focused {
            return Line::raw(last_chars(&chars, width));
        }
        return Line::from(input_spans(&chars, input.cursor(), width, focused, theme).0);
    }
    choice_line(&choice(form, field), focused, theme)
}

pub fn draw_export(frame: &mut Frame, form: &ExportForm, theme: &Theme) -> ExportLayout {
    let height = BORDER_ROWS + FIRST_FIELD_ROW + MAX_FIELDS + FOOTER_ROWS;
    let width = modal_width(frame.area().width, WIDTH);
    let area = centered(frame.area(), width, height);
    let SidebarModal { sidebar, pane, .. } =
        SidebarModal::open(frame, area, text::TITLE_EXPORT, theme);

    let formats = form.formats();
    let names: Vec<&str> = formats
        .iter()
        .map(|format| text::export_format_name(*format))
        .collect();
    let open = formats
        .iter()
        .position(|format| *format == form.format)
        .unwrap_or(0);
    let mut layout = ExportLayout {
        formats: draw_sidebar_rows(
            frame,
            sidebar,
            &names,
            (open, form.focus == Field::Formats),
            theme,
        ),
        fields: Vec::new(),
        browse: None,
    };

    for (y, field) in (pane.y + FIRST_FIELD_ROW..).zip(form.fields()) {
        let row = Rect {
            y,
            height: 1,
            ..pane
        };
        let is_path = field == Field::File;
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
            layout.browse = Some(draw_browse_button(frame, row, focused, theme));
        }
        layout.fields.push((field, row));
    }

    let content = Rect {
        x: pane.x + LEFT,
        y: pane.y,
        width: pane.width.saturating_sub(LEFT + RIGHT),
        height: 1,
    };
    let button_row = pane.bottom().saturating_sub(2);
    if let Some(note) = &form.note {
        draw_note(frame, content, button_row, note, theme.error);
    } else if let Some(reason) = form.all_rows_reason() {
        draw_note(frame, content, button_row, reason, theme.hint);
    }
    let buttons = [
        (Field::Export, text::BUTTON_EXPORT),
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
