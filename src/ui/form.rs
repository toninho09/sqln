//! The connection form.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::console::{centered, modal_width};
use super::float_block;
use super::icons::connection_symbol;
use super::input_spans;
use super::settings::{
    BORDER_ROWS, FOOTER_ROWS, LEFT, RIGHT, SIDEBAR_WIDTH, browse_width, draw_browse_button,
    draw_buttons, draw_divider, draw_note, draw_sidebar_rows,
};
use super::text;
use super::theme::Theme;
use crate::connector::target::hide_uri_password;
use crate::form::{ConnectionForm, Field, FieldStyle, FormNote, Group, Kind};
use crate::grid::printable;
use crate::store::Scope;

/// The sidebar, the label, and the longest choice fit in this width.
const WIDTH: u16 = 76;
const LABEL_WIDTH: u16 = 25;
/// The rows inside the window, from its top.
const TITLE_ROW: u16 = 1;
const FIRST_FIELD_ROW: u16 = 3;
const MASK: char = '•';

fn section_title(section: Group) -> &'static str {
    match section {
        Group::General => text::FORM_GENERAL,
        Group::Address => text::FORM_ADDRESS,
        Group::Credentials => text::FORM_CREDENTIALS,
        Group::Security => text::FORM_SECURITY,
        Group::Options => text::FORM_OPTIONS,
    }
}

/// The screen areas of the sidebar and the fields. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct FormLayout {
    pub sections: Vec<(Group, Rect)>,
    pub fields: Vec<(Field, Rect)>,
    /// The buttons of the path fields.
    pub browse: Vec<(Field, Rect)>,
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
        Field::Production => text::FIELD_PRODUCTION,
        Field::ConnectTimeout => text::FIELD_CONNECT,
        Field::SqlTimeout => text::FIELD_SQL_TIMEOUT,
        Field::IdleTimeout => text::FIELD_IDLE_TIMEOUT,
        Field::Theme => text::FIELD_CONNECTION_THEME,
        Field::Icon => text::FIELD_CONNECTION_ICON,
        Field::Test => text::BUTTON_TEST,
        Field::Save => text::BUTTON_SAVE,
        Field::Disconnect => text::BUTTON_DISCONNECT,
        Field::Cancel => text::BUTTON_CANCEL,
        Field::Sections => "",
    }
}

fn choice(form: &ConnectionForm, field: Field) -> &'static str {
    match field {
        Field::Kind => match form.kind {
            Kind::Postgres => text::VALUE_POSTGRES,
            Kind::MySql => text::VALUE_MYSQL,
            Kind::Redis => text::VALUE_REDIS,
            Kind::Sqlite => text::VALUE_SQLITE,
        },
        Field::Scope if form.scope == Scope::Project => text::VALUE_SCOPE_PROJECT,
        Field::Scope => text::VALUE_SCOPE_GLOBAL,
        Field::Entry if form.use_uri => text::VALUE_URI,
        Field::Entry => text::VALUE_FIELDS,
        Field::Credential if form.credential == crate::store::config::CredentialName::Vault => {
            text::VALUE_VAULT
        }
        Field::Credential => text::VALUE_SESSION,
        Field::Tls if form.verify_tls => text::VALUE_TLS_VERIFY,
        Field::Tls => text::VALUE_TLS_OFF,
        Field::Theme => form.theme.map_or(text::VALUE_DEFAULT, text::theme_name),
        Field::Icon => form
            .icon
            .map_or(text::VALUE_DEFAULT, text::connection_icon_name),
        _ => match form.foreign_keys {
            None => text::VALUE_DEFAULT,
            Some(true) => text::VALUE_ON,
            Some(false) => text::VALUE_OFF,
        },
    }
}

/// Writes a text value, and it keeps the cursor inside the width.
fn text_spans(
    form: &ConnectionForm,
    field: Field,
    width: usize,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = match field {
        Field::Password => form.text(field).chars().map(|_| MASK).collect(),
        Field::Uri => hide_uri_password(&printable(form.text(field)), MASK)
            .chars()
            .collect(),
        _ => printable(form.text(field)).chars().collect(),
    };
    input_spans(&chars, form.cursor, width, form.focus == field, theme).0
}

fn value_spans(
    form: &ConnectionForm,
    field: Field,
    width: usize,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let style = if form.focus == field {
        theme.dialog_focus
    } else {
        Style::new()
    };
    match field.style() {
        FieldStyle::Choice => {
            let symbol = form
                .icon
                .filter(|_| field == Field::Icon)
                .map_or(String::new(), |icon| Theme::icon(connection_symbol(icon)));
            let name = choice(form, field);
            vec![Span::styled(format!("< {symbol}{name} >"), style)]
        }
        FieldStyle::Flag => {
            let mark = if form.flag(field) { "[x]" } else { "[ ]" };
            vec![Span::styled(mark, style)]
        }
        _ => text_spans(form, field, width, theme),
    }
}

/// Draws one row: the focus mark, the label, and the value. It returns the area of the
/// button of a path field.
fn draw_field_row(
    frame: &mut Frame,
    area: Rect,
    form: &ConnectionForm,
    field: Field,
    theme: &Theme,
) -> Option<Rect> {
    let focused = form.focus == field;
    if focused {
        let mark = Span::styled(theme.icons.focus_mark, theme.panel_title_focused);
        frame.render_widget(
            Paragraph::new(mark),
            Rect {
                x: area.x + 1,
                width: 1,
                ..area
            },
        );
    }
    let label_style = if focused {
        theme.panel_title_focused
    } else {
        Style::new()
    };
    let label_area = Rect {
        x: area.x + LEFT,
        width: LABEL_WIDTH,
        ..area
    };
    frame.render_widget(Paragraph::new(label(field)).style(label_style), label_area);

    let start = area.x + LEFT + LABEL_WIDTH;
    let value = Rect {
        x: start,
        width: area.right().saturating_sub(start + RIGHT),
        ..area
    };
    // The background comes first, because the text and the cursor have their own styles.
    frame.buffer_mut().set_style(
        value,
        if focused {
            theme.field_focus
        } else {
            theme.field
        },
    );
    let text = Rect {
        x: value.x + 1,
        width: value.width.saturating_sub(2),
        ..value
    };
    let text = Rect {
        width: text.width.saturating_sub(if field.is_path() {
            browse_width(theme)
        } else {
            0
        }),
        ..text
    };
    let spans = value_spans(form, field, usize::from(text.width).max(1), theme);
    frame.render_widget(Paragraph::new(Line::from(spans)), text);
    field
        .is_path()
        .then(|| draw_browse_button(frame, area, focused, theme))
}

pub fn draw_form(frame: &mut Frame, form: &ConnectionForm, theme: &Theme) -> FormLayout {
    let sections = form.sections();
    // All sections keep the size of the largest one, so the window does not move.
    let tallest = sections
        .iter()
        .map(|section| form.fields_of(*section).len())
        .max()
        .unwrap_or(0);
    let height =
        BORDER_ROWS + FIRST_FIELD_ROW + u16::try_from(tallest).unwrap_or(u16::MAX) + FOOTER_ROWS;
    let area = centered(frame.area(), modal_width(frame.area().width, WIDTH), height);
    frame.render_widget(Clear, area);
    let title = match &form.editing {
        None => text::TITLE_NEW_CONNECTION.to_owned(),
        Some(_) => text::edit_connection_title(&printable(form.text(Field::Name))),
    };
    let block = float_block(&title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

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
    let open = form.shown_section();
    let titles: Vec<&str> = sections
        .iter()
        .map(|section| section_title(*section))
        .collect();
    let index = sections.iter().position(|section| *section == open);
    let focused = form.focus == Field::Sections;
    let rows = draw_sidebar_rows(
        frame,
        sidebar,
        &titles,
        (index.unwrap_or(0), focused),
        theme,
    );
    let mut layout = FormLayout {
        sections: sections.into_iter().zip(rows).collect(),
        ..FormLayout::default()
    };

    let content = Rect {
        x: pane.x + LEFT,
        y: pane.y + TITLE_ROW,
        width: pane.width.saturating_sub(LEFT + RIGHT),
        height: 1,
    };
    let heading = section_title(open).to_uppercase();
    frame.render_widget(Paragraph::new(heading).style(theme.panel_title), content);
    for (y, field) in (pane.y + FIRST_FIELD_ROW..).zip(form.section_fields()) {
        let row = Rect {
            y,
            height: 1,
            ..pane
        };
        if let Some(button) = draw_field_row(frame, row, form, field, theme) {
            layout.browse.push((field, button));
        }
        layout.fields.push((field, row));
    }

    let button_row = pane.bottom().saturating_sub(2);
    if let Some(note) = &form.note {
        let (message, style) = match note {
            FormNote::Info(message) => (message, theme.status),
            FormNote::Success(message) => (message, theme.mark_good),
            FormNote::Error(message) => (message, theme.error),
        };
        draw_note(frame, content, button_row, message, style);
    }
    let buttons: Vec<(Field, &str)> = form
        .buttons()
        .into_iter()
        .map(|field| (field, label(field)))
        .collect();
    layout.fields.extend(draw_buttons(
        frame,
        content,
        button_row,
        (&buttons, form.focus),
        theme,
    ));
    layout
}
