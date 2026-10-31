//! The connection form.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::console::centered;
use super::text;
use super::input_spans;
use super::theme::Theme;
use crate::connector::target::hide_uri_password;
use crate::form::{ConnectionForm, Field, FieldStyle, FormNote, Kind};
use crate::grid::printable;

const WIDTH: u16 = 68;
const LABEL_WIDTH: usize = 24;
const NOTE_ROWS: u16 = 2;
const MASK: char = '•';

/// The screen areas of the fields. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct FormLayout {
    pub fields: Vec<(Field, Rect)>,
}

fn label(field: Field) -> &'static str {
    match field {
        Field::Kind => text::FIELD_KIND,
        Field::Name => text::FIELD_NAME,
        Field::Entry => text::FIELD_ENTRY,
        Field::Uri => text::FIELD_URI,
        Field::Host => text::FIELD_HOST,
        Field::Port => text::FIELD_PORT,
        Field::Database => text::FIELD_DATABASE,
        Field::User => text::FIELD_USER,
        Field::Password => text::FIELD_PASSWORD,
        Field::Tls => text::FIELD_TLS,
        Field::CaFile => text::FIELD_CA,
        Field::ClientCertificate => text::FIELD_CLIENT_CERTIFICATE,
        Field::ClientKey => text::FIELD_CLIENT_KEY,
        Field::File => text::FIELD_FILE,
        Field::ForeignKeys => text::FIELD_FOREIGN_KEYS,
        Field::BusyTimeout => text::FIELD_BUSY,
        Field::Pool => text::FIELD_POOL,
        Field::ReadOnly => text::FIELD_READ_ONLY,
        Field::ConnectTimeout => text::FIELD_CONNECT,
        Field::Test => text::BUTTON_TEST,
        Field::Save => text::BUTTON_SAVE,
        Field::Disconnect => text::BUTTON_DISCONNECT,
        Field::Cancel => text::BUTTON_CANCEL,
    }
}

fn choice(form: &ConnectionForm, field: Field) -> &'static str {
    match field {
        Field::Kind => match form.kind {
            Kind::Postgres => text::VALUE_POSTGRES,
            Kind::MySql => text::VALUE_MYSQL,
            Kind::Sqlite => text::VALUE_SQLITE,
        },
        Field::Entry if form.use_uri => text::VALUE_URI,
        Field::Entry => text::VALUE_FIELDS,
        Field::Tls if form.verify_tls => text::VALUE_TLS_VERIFY,
        Field::Tls => text::VALUE_TLS_OFF,
        _ => match form.foreign_keys {
            None => text::VALUE_DEFAULT,
            Some(true) => text::VALUE_ON,
            Some(false) => text::VALUE_OFF,
        },
    }
}

/// Writes a text value, and it keeps the cursor inside the width.
fn text_spans(form: &ConnectionForm, field: Field, width: usize, theme: &Theme) -> Vec<Span<'static>> {
    let chars: Vec<char> = match field {
        Field::Password => form.text(field).chars().map(|_| MASK).collect(),
        Field::Uri => hide_uri_password(&printable(form.text(field)), MASK).chars().collect(),
        _ => printable(form.text(field)).chars().collect(),
    };
    input_spans(&chars, form.cursor, width, form.focus == field, theme).0
}

fn value_spans(form: &ConnectionForm, field: Field, width: usize, theme: &Theme) -> Vec<Span<'static>> {
    let focus = if form.focus == field { theme.dialog_focus } else { theme.dialog_option };
    match field.style() {
        FieldStyle::Choice => vec![Span::styled(format!("< {} >", choice(form, field)), focus)],
        FieldStyle::Flag => {
            let mark = if form.read_only { "[x]" } else { "[ ]" };
            vec![Span::styled(mark, focus)]
        }
        _ => text_spans(form, field, width, theme),
    }
}

fn draw_field_row(frame: &mut Frame, area: Rect, form: &ConnectionForm, field: Field, theme: &Theme) {
    let width = usize::from(area.width).saturating_sub(LABEL_WIDTH + 1).max(1);
    let mut spans = vec![Span::raw(format!("{:<LABEL_WIDTH$}", label(field)))];
    spans.extend(value_spans(form, field, width, theme));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn button_label(form: &ConnectionForm, field: Field) -> &'static str {
    if field == Field::Save && form.name_only {
        return text::BUTTON_SAVE_NAME;
    }
    label(field)
}

pub fn draw_form(frame: &mut Frame, form: &ConnectionForm, theme: &Theme) -> FormLayout {
    let fields = form.fields();
    let (buttons, rows): (Vec<Field>, Vec<Field>) =
        fields.into_iter().partition(|field| field.style() == FieldStyle::Button);
    let height = u16::try_from(rows.len()).unwrap_or(0) + NOTE_ROWS + 3;
    let area = centered(frame.area(), WIDTH, height);
    frame.render_widget(Clear, area);
    let title = match (&form.editing, form.name_only) {
        (None, _) => text::TITLE_NEW_CONNECTION.to_owned(),
        _ => text::edit_connection_title(&printable(form.text(Field::Name))),
    };
    let block = Block::bordered().title(format!(" {title} ")).style(theme.dialog);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut layout = FormLayout::default();
    let mut y = inner.y;
    for field in rows {
        let row = Rect { y, height: 1, ..inner };
        draw_field_row(frame, row, form, field, theme);
        layout.fields.push((field, row));
        y += 1;
    }
    let note_area = Rect { y, height: NOTE_ROWS, ..inner };
    if let Some(note) = &form.note {
        let (message, style) = match note {
            FormNote::Info(message) => (message, theme.status),
            FormNote::Error(message) => (message, theme.error),
        };
        let paragraph = Paragraph::new(Line::styled(printable(message), style)).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, note_area);
    }
    y += NOTE_ROWS;

    let mut x = inner.x;
    let mut spans = Vec::new();
    for field in buttons {
        let name = format!("[{}]", button_label(form, field));
        let width = u16::try_from(name.width()).unwrap_or(0);
        layout.fields.push((field, Rect::new(x, y, width, 1)));
        let style: Style = if form.focus == field { theme.dialog_focus } else { theme.dialog_option };
        spans.push(Span::styled(name, style));
        spans.push(Span::raw("  "));
        x += width + 2;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), Rect { y, height: 1, ..inner });
    layout
}
