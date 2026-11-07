//! The connection form.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::icons::connection_symbol;
use super::kit::buttons::{self, RowOptions};
use super::kit::feedback::draw_note;
use super::kit::form::{
    browse_width, choice_value, draw_browse_button, draw_row, flag_mark, focus_style, value_width,
};
use super::kit::geometry::modal_width;
use super::kit::input::{self, FieldMode, FieldText};
use super::kit::list::{ListWindow, draw_list_window};
use super::kit::modal::{Modal, SidebarSpec};
use super::kit::style::{self, ListRow, NoteLevel};
use super::text;
use super::theme::Theme;
use crate::connector::target::hide_uri_password;
use crate::filter_list::FilterList;
use crate::form::{ConnectionForm, Field, FieldStyle, FormNote, Group, Kind, SchemaList};
use crate::grid::printable;
use crate::store::Scope;

/// The sidebar, the label, and the longest choice fit in this width.
const WIDTH: u16 = 76;
const LABEL_WIDTH: u16 = super::kit::form::LabelWidth::Wide.columns();
/// The rows inside the window, from its top.
const TITLE_ROW: u16 = 1;
const FIRST_FIELD_ROW: u16 = 3;
const MASK: char = '•';
const LIST_WIDTH: u16 = 60;

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
    /// The buttons of the fields that open a window.
    pub browse: Vec<(Field, Rect)>,
    /// The rows of the list of the schemas and the index of its first row.
    pub list: (Rect, usize),
}

fn label(kind: Kind, field: Field) -> &'static str {
    match field {
        Field::Schemas if kind == Kind::Postgres => text::FIELD_SCHEMAS,
        Field::Schemas => text::FIELD_DATABASES,
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
fn text_line(form: &ConnectionForm, field: Field, width: usize, theme: &Theme) -> Line<'static> {
    let text = match field {
        Field::Password => form.text(field).to_owned(),
        Field::Uri => hide_uri_password(&printable(form.text(field)), MASK),
        _ => printable(form.text(field)),
    };
    let mode = match field {
        Field::Password => FieldMode::Masked {
            mask: MASK,
            limit: None,
        },
        Field::Uri => FieldMode::Path,
        _ => FieldMode::Text,
    };
    input::line(
        FieldText {
            text: &text,
            cursor: form.cursor,
            width,
            focused: form.focus == field,
            mode,
        },
        theme,
    )
}

fn value_line(form: &ConnectionForm, field: Field, width: usize, theme: &Theme) -> Line<'static> {
    let style = focus_style(form.focus == field, theme);
    match field.style() {
        FieldStyle::Choice => {
            let symbol = form
                .icon
                .filter(|_| field == Field::Icon)
                .map_or(String::new(), |icon| Theme::icon(connection_symbol(icon)));
            Line::from(Span::styled(
                choice_value(&format!("{symbol}{}", choice(form, field))),
                style,
            ))
        }
        FieldStyle::Flag => Line::from(Span::styled(flag_mark(form.flag(field)), style)),
        _ => text_line(form, field, width, theme),
    }
}

/// Draws one row: the focus mark, the label, and the value. It returns the area of the
/// button of a field that opens a window.
fn draw_field_row(
    frame: &mut Frame,
    area: Rect,
    form: &ConnectionForm,
    field: Field,
    theme: &Theme,
) -> Option<Rect> {
    let focused = form.focus == field;
    let mut width = value_width(area, LABEL_WIDTH);
    if field.opens_window() {
        width = width.saturating_sub(usize::from(browse_width(theme)));
    }
    let line = value_line(form, field, width.max(1), theme);
    draw_row(
        frame,
        area,
        (label(form.kind, field), LABEL_WIDTH),
        focused,
        line,
        theme,
    );
    field
        .opens_window()
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
    let title = match &form.editing {
        None => text::TITLE_NEW_CONNECTION.to_owned(),
        Some(_) => text::edit_connection_title(&printable(form.text(Field::Name))),
    };
    let open = form.shown_section();
    let titles: Vec<&str> = sections
        .iter()
        .map(|section| section_title(*section))
        .collect();
    let index = sections
        .iter()
        .position(|section| *section == open)
        .unwrap_or(0);
    let modal = Modal::form(
        modal_width(frame.area().width, WIDTH),
        FIRST_FIELD_ROW,
        u16::try_from(tallest).unwrap_or(u16::MAX),
    )
    .title(title)
    .heading_row(TITLE_ROW)
    .sidebar(SidebarSpec {
        titles: &titles,
        open: index,
        focused: form.focus == Field::Sections,
    })
    .open(frame, theme);
    let pane = modal.pane;
    let mut layout = FormLayout {
        sections: sections.into_iter().zip(modal.sidebar_rows).collect(),
        ..FormLayout::default()
    };

    let content = modal.content;
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

    let button_row = modal.button_row;
    if let Some(note) = &form.note {
        let (message, level) = match note {
            FormNote::Info(message) => (message, NoteLevel::Info),
            FormNote::Success(message) => (message, NoteLevel::Success),
            FormNote::Error(message) => (message, NoteLevel::Error),
        };
        draw_note(
            frame,
            content,
            button_row,
            message,
            style::note_style(level, theme),
        );
    }
    let buttons = buttons::dialog_items(
        form.buttons()
            .into_iter()
            .map(|field| (field, label(form.kind, field))),
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
    if let Some(open) = &form.schema_list {
        layout.list = draw_schema_list(frame, form.kind, open, theme);
    }
    layout
}

/// Draws the list of the schemas over the form. It returns the area of the rows and the
/// index of the first row.
fn draw_schema_list(
    frame: &mut Frame,
    kind: Kind,
    open: &SchemaList,
    theme: &Theme,
) -> (Rect, usize) {
    let window = ListWindow {
        title: label(kind, Field::Schemas),
        empty: if open.loading {
            text::SCHEMA_LIST_READING
        } else {
            text::NO_SCHEMA_MATCH
        },
        width: LIST_WIDTH,
    };
    // The marked names wait for the names of the server.
    let waiting = FilterList::new(Vec::new(), String::clone);
    let list = if open.loading {
        &waiting
    } else {
        open.list.list()
    };
    draw_list_window(
        frame,
        &window,
        list,
        theme,
        |frame, area, name, selected| {
            let style = style::list_row_style(
                if selected {
                    ListRow::Selected
                } else {
                    ListRow::Normal
                },
                theme,
            );
            frame.buffer_mut().set_style(area, style);
            let mark = flag_mark(open.list.is_marked(name));
            let line = format!(" {mark} {}", printable(name));
            frame.render_widget(Paragraph::new(line), area);
        },
    )
}
