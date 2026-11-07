//! The export dialog.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::kit::buttons::{self, RowOptions};
use super::kit::feedback::draw_note;
use super::kit::form::{browse_width, choice_line, draw_browse_button, draw_row, value_width};
use super::kit::geometry::modal_width;
use super::kit::input::{self, FieldMode, FieldText};
use super::kit::modal::{Modal, SidebarSpec};
use super::settings::WIDTH;
use super::text;
use super::theme::Theme;
use crate::export::NullText;
use crate::export::form::{ExportForm, Field, RowChoice};

/// The columns of a label. `Rows per INSERT` needs them, with one space.
const LABEL_WIDTH: u16 = super::kit::form::LabelWidth::Narrow.columns();
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

fn value(form: &ExportForm, field: Field, width: usize, theme: &Theme) -> Line<'static> {
    let focused = form.focus == field;
    if let Some(input) = form.input(field) {
        let mode = if field == Field::File {
            FieldMode::Path
        } else {
            FieldMode::Text
        };
        return input::line(
            FieldText {
                text: input.text(),
                cursor: input.cursor(),
                width,
                focused,
                mode,
            },
            theme,
        );
    }
    choice_line(&choice(form, field), focused, theme)
}

pub fn draw_export(frame: &mut Frame, form: &ExportForm, theme: &Theme) -> ExportLayout {
    let formats = form.formats();
    let names: Vec<&str> = formats
        .iter()
        .map(|format| text::export_format_name(*format))
        .collect();
    let open = formats
        .iter()
        .position(|format| *format == form.format)
        .unwrap_or(0);
    let modal = Modal::form(
        modal_width(frame.area().width, WIDTH),
        FIRST_FIELD_ROW,
        MAX_FIELDS,
    )
    .title(text::TITLE_EXPORT)
    .sidebar(SidebarSpec {
        titles: &names,
        open,
        focused: form.focus == Field::Formats,
    })
    .open(frame, theme);
    let pane = modal.pane;
    let mut layout = ExportLayout {
        formats: modal.sidebar_rows,
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

    let content = modal.content;
    let button_row = modal.button_row;
    if let Some(note) = &form.note {
        draw_note(frame, content, button_row, note, theme.error);
    } else if let Some(reason) = form.all_rows_reason() {
        draw_note(frame, content, button_row, reason, theme.hint);
    }
    let buttons = buttons::dialog_items(
        [
            (Field::Export, text::BUTTON_EXPORT),
            (Field::Cancel, text::OPTION_CANCEL),
        ],
        form.focus,
        theme,
    );
    layout.fields.extend(buttons::draw(
        frame,
        Rect {
            y: button_row,
            height: 1,
            ..content
        },
        RowOptions::DIALOG,
        &buttons,
        theme,
    ));
    layout
}
