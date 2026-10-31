//! The connection form.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::console::centered;
use super::float_block;
use super::text;
use super::input_spans;
use super::theme::Theme;
use crate::connector::target::hide_uri_password;
use crate::form::{ConnectionForm, Field, FieldStyle, FormNote, Kind};
use crate::grid::printable;
use crate::store::Scope;

const WIDTH: u16 = 72;
/// The window with two columns. Each value keeps 26 columns, the width of the longest choice.
const WIDE_WIDTH: u16 = 116;
const LABEL_WIDTH: u16 = 24;
const NOTE_ROWS: u16 = 2;
/// The columns before a label: a space, the focus mark, and a space.
const LEFT: u16 = 3;
/// The empty columns after a value.
const RIGHT: u16 = 2;
/// The rows of the window without the fields: the border, the notes, and the buttons.
const CHROME_ROWS: u16 = NOTE_ROWS + 3;
/// The free rows and columns around the window, when the window has room to choose.
const MARGIN: u16 = 2;
const MASK: char = '•';

/// One way to place the fields. The first way that fits the screen applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spacing {
    two_columns: bool,
    titles: bool,
    gaps: bool,
}

const COMPACT: Spacing = Spacing { two_columns: false, titles: false, gaps: false };
const SPACINGS: [Spacing; 4] = [
    Spacing { two_columns: false, titles: true, gaps: true },
    Spacing { two_columns: true, titles: true, gaps: true },
    Spacing { two_columns: false, titles: false, gaps: true },
    COMPACT,
];

impl Spacing {
    /// Divides the groups into columns. The first column stops after half of the fields.
    fn columns<'a>(self, groups: &[&'a [Field]]) -> Vec<Vec<&'a [Field]>> {
        if !self.two_columns {
            return vec![groups.to_vec()];
        }
        let half = groups.iter().map(|group| group.len()).sum::<usize>().div_ceil(2);
        let mut used = 0;
        let (mut first, mut second) = (Vec::new(), Vec::new());
        for group in groups {
            if used < half {
                first.push(*group);
                used += group.len();
            } else {
                second.push(*group);
            }
        }
        vec![first, second]
    }

    fn rows_of(self, column: &[&[Field]]) -> u16 {
        let fields: usize = column.iter().map(|group| group.len()).sum();
        let titles = if self.titles { column.len() } else { 0 };
        let gaps = if self.gaps { column.len().saturating_sub(1) } else { 0 };
        u16::try_from(fields + titles + gaps).unwrap_or(u16::MAX)
    }

    /// Returns the width and the height of the window.
    fn size(self, groups: &[&[Field]]) -> (u16, u16) {
        let rows = self.columns(groups).iter().map(|column| self.rows_of(column)).max().unwrap_or(0);
        let width = if self.two_columns { WIDE_WIDTH } else { WIDTH };
        (width, rows + u16::from(self.gaps) * 2 + CHROME_ROWS)
    }

    fn choose(screen: Rect, groups: &[&[Field]]) -> Self {
        let fits = |spacing: &Self| {
            let (width, height) = spacing.size(groups);
            width + MARGIN <= screen.width && height + MARGIN <= screen.height
        };
        SPACINGS.into_iter().find(fits).unwrap_or(COMPACT)
    }
}

/// The groups of the form, in the order of its fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    General,
    Address,
    Credentials,
    Security,
    Options,
}

fn group(field: Field) -> Group {
    match field {
        Field::Kind | Field::Name | Field::Scope | Field::Entry => Group::General,
        Field::Uri | Field::Host | Field::Port | Field::Database | Field::File => Group::Address,
        Field::User | Field::Password | Field::Credential => Group::Credentials,
        Field::Tls | Field::CaFile | Field::ClientCertificate | Field::ClientKey => Group::Security,
        _ => Group::Options,
    }
}

fn group_title(group: Group) -> &'static str {
    match group {
        Group::General => text::FORM_GENERAL,
        Group::Address => text::FORM_ADDRESS,
        Group::Credentials => text::FORM_CREDENTIALS,
        Group::Security => text::FORM_SECURITY,
        Group::Options => text::FORM_OPTIONS,
    }
}

/// The screen areas of the fields. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct FormLayout {
    pub fields: Vec<(Field, Rect)>,
}

fn label(field: Field) -> &'static str {
    match field {
        Field::Kind => text::FIELD_KIND,
        Field::Name => text::FIELD_NAME,
        Field::Scope => text::FIELD_SCOPE,
        Field::Entry => text::FIELD_ENTRY,
        Field::Uri => text::FIELD_URI,
        Field::Host => text::FIELD_HOST,
        Field::Port => text::FIELD_PORT,
        Field::Database => text::FIELD_DATABASE,
        Field::User => text::FIELD_USER,
        Field::Password => text::FIELD_PASSWORD,
        Field::Credential => text::FIELD_CREDENTIAL,
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
        Field::SqlTimeout => text::FIELD_SQL_TIMEOUT,
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
        Field::Scope if form.scope == Scope::Project => text::VALUE_SCOPE_PROJECT,
        Field::Scope => text::VALUE_SCOPE_GLOBAL,
        Field::Entry if form.use_uri => text::VALUE_URI,
        Field::Entry => text::VALUE_FIELDS,
        Field::Credential if form.credential == crate::store::config::CredentialName::Vault => text::VALUE_VAULT,
        Field::Credential => text::VALUE_SESSION,
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
    let style = if form.focus == field { theme.dialog_focus } else { Style::new() };
    match field.style() {
        FieldStyle::Choice => vec![Span::styled(format!("< {} >", choice(form, field)), style)],
        FieldStyle::Flag => {
            let mark = if form.read_only { "[x]" } else { "[ ]" };
            vec![Span::styled(mark, style)]
        }
        _ => text_spans(form, field, width, theme),
    }
}

/// Draws one row: the focus mark, the label, and the value.
fn draw_field_row(frame: &mut Frame, area: Rect, form: &ConnectionForm, field: Field, theme: &Theme) {
    let focused = form.focus == field;
    if focused {
        let mark = Span::styled(theme.icons.focus_mark, theme.panel_title_focused);
        frame.render_widget(Paragraph::new(mark), Rect { x: area.x + 1, width: 1, ..area });
    }
    let label_style = if focused { theme.panel_title_focused } else { Style::new() };
    let label_area = Rect { x: area.x + LEFT, width: LABEL_WIDTH, ..area };
    frame.render_widget(Paragraph::new(label(field)).style(label_style), label_area);

    let start = area.x + LEFT + LABEL_WIDTH;
    let value = Rect { x: start, width: area.right().saturating_sub(start + RIGHT), ..area };
    // The background comes first, because the text and the cursor have their own styles.
    frame.buffer_mut().set_style(value, if focused { theme.field_focus } else { theme.field });
    let text = Rect { x: value.x + 1, width: value.width.saturating_sub(2), ..value };
    let spans = value_spans(form, field, usize::from(text.width).max(1), theme);
    frame.render_widget(Paragraph::new(Line::from(spans)), text);
}

/// Draws the groups of one column, from the top of `area`.
fn draw_column(
    frame: &mut Frame,
    area: Rect,
    column: &[&[Field]],
    spacing: Spacing,
    form: &ConnectionForm,
    theme: &Theme,
    layout: &mut FormLayout,
) {
    let mut y = area.y;
    for (index, fields) in column.iter().enumerate() {
        if index > 0 && spacing.gaps {
            y += 1;
        }
        if spacing.titles {
            let title = fields.first().map_or("", |field| group_title(group(*field)));
            let place = Rect { x: area.x + LEFT, y, width: area.width.saturating_sub(LEFT), height: 1 };
            frame.render_widget(Paragraph::new(title).style(theme.panel_title), place);
            y += 1;
        }
        for field in *fields {
            let row = Rect { y, height: 1, ..area };
            draw_field_row(frame, row, form, *field, theme);
            layout.fields.push((*field, row));
            y += 1;
        }
    }
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
    let groups: Vec<&[Field]> = rows.chunk_by(|first, next| group(*first) == group(*next)).collect();
    let spacing = Spacing::choose(frame.area(), &groups);
    let (width, height) = spacing.size(&groups);
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);
    let title = match (&form.editing, form.name_only) {
        (None, _) => text::TITLE_NEW_CONNECTION.to_owned(),
        _ => text::edit_connection_title(&printable(form.text(Field::Name))),
    };
    let block = float_block(&title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let pad = u16::from(spacing.gaps);
    let body = Rect { y: inner.y + pad, height: inner.height.saturating_sub(pad * 2), ..inner };
    let mut layout = FormLayout::default();
    let columns = spacing.columns(&groups);
    let column_width = body.width / u16::try_from(columns.len()).unwrap_or(1);
    for (index, column) in (0u16..).zip(&columns) {
        let place = Rect { x: body.x + index * column_width, width: column_width, ..body };
        draw_column(frame, place, column, spacing, form, theme, &mut layout);
    }

    let button_row = body.bottom().saturating_sub(1);
    let content = Rect {
        x: body.x + LEFT,
        y: button_row.saturating_sub(NOTE_ROWS),
        width: body.width.saturating_sub(LEFT + RIGHT),
        height: NOTE_ROWS,
    };
    if let Some(note) = &form.note {
        let (message, style) = match note {
            FormNote::Info(message) => (message, theme.status),
            FormNote::Success(message) => (message, theme.mark_good),
            FormNote::Error(message) => (message, theme.error),
        };
        let message = printable(message);
        // The note ends on the row above the buttons, and a long note goes up to the next row.
        let width = usize::from(content.width).max(1);
        let rows = u16::try_from(message.width().div_ceil(width)).unwrap_or(NOTE_ROWS).clamp(1, NOTE_ROWS);
        let place = Rect { y: button_row.saturating_sub(rows), height: rows, ..content };
        let paragraph = Paragraph::new(Line::styled(message, style)).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, place);
    }

    let mut x = content.x;
    let mut spans = Vec::new();
    for field in buttons {
        let name = theme.button(button_label(form, field));
        let width = u16::try_from(name.width()).unwrap_or(0);
        layout.fields.push((field, Rect::new(x, button_row, width, 1)));
        let style: Style = if form.focus == field { theme.dialog_focus } else { theme.dialog_option };
        spans.push(Span::styled(name, style));
        spans.push(Span::raw("  "));
        x += width + 2;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), Rect { y: button_row, height: 1, ..content });
    layout
}
