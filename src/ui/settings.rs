//! The settings dialog.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::kit::buttons::{self, RowOptions};
use super::kit::feedback::draw_note;
use super::kit::form::{choice_line, draw_row, value_width};
use super::kit::geometry::{BORDER_ROWS, FOOTER_ROWS, NOTE_ROWS, modal_width};
use super::kit::input::{self, FieldMode, FieldText};
use super::kit::list::viewport_centered;
use super::kit::measure::fit;
use super::kit::modal::{Modal, SidebarSpec};
use super::text;
use super::theme::Theme;
use crate::input::Action;
use crate::settings::{Field, SECTIONS, SettingsForm};
use crate::store::Scope;

use ratatui::style::Style;

pub(super) const WIDTH: u16 = 72;
const LABEL_WIDTH: u16 = super::kit::form::LabelWidth::Narrow.columns();
/// The value columns of a number field.
const NUMBER_WIDTH: usize = 12;
/// The value columns of the hex text of a color.
const HEX_WIDTH: usize = 9;
/// The rows inside the window, from its top.
const SCOPE_ROW: u16 = 1;
const TITLE_ROW: u16 = 3;
const FIRST_FIELD_ROW: u16 = 4;

/// The screen areas of the sidebar and the fields. The mouse uses them.
#[derive(Clone, Debug, Default)]
pub struct SettingsLayout {
    /// One area for each row of `SECTIONS`, in the same order.
    pub sections: Vec<Rect>,
    pub fields: Vec<(Field, Rect)>,
    pub command_rows: Vec<(Action, Rect)>,
    /// One area for each theme of the list, with its id.
    pub theme_rows: Vec<(String, Rect)>,
    /// One area for each color of the editor.
    pub color_rows: Vec<(usize, Rect)>,
}

/// The window has the height of the largest section, and all sections keep the same size.
fn window_height() -> u16 {
    let fields = SECTIONS.iter().map(|section| section.fields.len()).max();
    let fields = u16::try_from(fields.unwrap_or(0)).unwrap_or(u16::MAX);
    BORDER_ROWS + FIRST_FIELD_ROW + fields + FOOTER_ROWS
}

fn label(field: Field) -> &'static str {
    match field {
        Field::Scope => text::FIELD_SCOPE,
        Field::Keyboard => text::FIELD_KEYBOARD,
        Field::PageSize => text::FIELD_PAGE_SIZE,
        Field::RowCap => text::FIELD_ROW_CAP,
        Field::CapInSql => text::FIELD_CAP_IN_SQL,
        Field::Budget => text::FIELD_BUDGET,
        Field::ReopenTabs => text::FIELD_REOPEN_TABS,
        Field::QueryHistory => text::FIELD_QUERY_HISTORY,
        Field::BellAfter => text::FIELD_BELL_AFTER,
        Field::FormatJson => text::FIELD_FORMAT_JSON,
        Field::ShowSystemCommands => text::FIELD_SHOW_SYSTEM_COMMANDS,
        Field::SuggestionKeys => text::FIELD_SUGGESTION_KEYS,
        Field::UndoLimit => text::FIELD_UNDO_LIMIT,
        Field::Theme => text::FIELD_THEME,
        Field::Themes => text::FIELD_THEMES,
        Field::ThemeName => text::FIELD_THEME_NAME,
        Field::ThemeBase => text::FIELD_THEME_BASE,
        Field::ShortcutMode => text::FIELD_SHORTCUT_MODE,
        Field::ShortcutBinding => text::FIELD_SHORTCUT_BINDING,
        Field::ShortcutFinish => text::FIELD_SHORTCUT_FINISH,
        Field::Mcp => text::FIELD_MCP,
        Field::McpDefault => text::FIELD_MCP_DEFAULT,
        Field::McpMax => text::FIELD_MCP_MAX,
        Field::McpRemember => text::FIELD_MCP_REMEMBER,
        Field::AiEnabled => text::FIELD_AI_ENABLED,
        Field::AiProfile => text::FIELD_AI_PROVIDER,
        Field::AiModel => text::FIELD_AI_MODEL,
        Field::AiEndpoint => text::FIELD_AI_ENDPOINT,
        Field::AiSource => text::FIELD_AI_SOURCE,
        Field::AiPath => text::FIELD_AI_PATH,
        Field::AiKey => text::FIELD_AI_KEY,
        Field::AiContext => text::FIELD_AI_CONTEXT,
        Field::AiOutput => text::FIELD_AI_OUTPUT,
        Field::AiTimeout => text::FIELD_AI_TIMEOUT,
        Field::AiReasoning => text::FIELD_AI_REASONING,
        Field::AiSimultaneous => text::FIELD_AI_SIMULTANEOUS,
        Field::AiTest => "Test",
        _ => "",
    }
}

/// Returns the text of a choice.
fn choice(form: &SettingsForm, field: Field) -> String {
    match field {
        Field::AiEnabled => form.ai_enabled().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::AiProfile => if form.ai_inherit() {
            "Inherited"
        } else {
            "Custom"
        }
        .into(),
        Field::AiSource => ["None", "System vault", "Key file"][form.ai_source()].into(),
        Field::AiReasoning => {
            ["Disabled", "None", "Low", "Medium", "High"][form.ai_reasoning()].into()
        }
        Field::AiKey => form.ai_key(),
        Field::AiTest => {
            if form.ai_busy {
                "Working…".to_owned()
            } else {
                text::BUTTON_TEST.to_owned()
            }
        }
        Field::Scope if form.scope == Scope::Project => text::VALUE_SCOPE_PROJECT.to_owned(),
        Field::Scope => text::VALUE_SCOPE_GLOBAL.to_owned(),
        Field::Keyboard => form
            .keyboard()
            .map_or_else(|| form.inherited(field), text::keyboard_name),
        Field::ReopenTabs => form.reopen_tabs().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::QueryHistory => form.query_history().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::ShowSystemCommands => form.show_system_commands().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::FormatJson => form.format_json().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::CapInSql => form.cap_in_sql().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::SuggestionKeys => form.suggestion_keys().map_or_else(
            || form.inherited(field),
            |keys| text::suggestion_keys_name(keys).to_owned(),
        ),
        Field::Theme => form
            .theme()
            .map_or_else(|| form.inherited(field), |name| form.theme_label(&name)),
        Field::Mcp => form.mcp().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::McpDefault => form.mcp_default().map_or_else(
            || form.inherited(field),
            |value| text::agent_default_name(value).to_owned(),
        ),
        Field::McpMax => form.mcp_max().map_or_else(
            || form.inherited(field),
            |value| text::agent_access_name(value).to_owned(),
        ),
        Field::McpRemember => form.mcp_remember().map_or_else(
            || form.inherited(field),
            |value| text::on_off(value).to_owned(),
        ),
        Field::ThemeNew
        | Field::ThemeEdit
        | Field::ThemeDelete
        | Field::ThemeDone
        | Field::ThemeBack
        | Field::ThemeBase
        | Field::Themes
        | Field::ThemeColors => form.theme_value(field),
        field => form.shortcut_value(field),
    }
}

fn value(form: &SettingsForm, field: Field, theme: &Theme, width: usize) -> Line<'static> {
    let focused = form.focus == field;
    if let Some(input) = form.input(field) {
        let mode = if field == Field::AiKey && !input.text().is_empty() {
            FieldMode::Masked {
                mask: '*',
                limit: Some(24),
            }
        } else {
            FieldMode::Text
        };
        // A number keeps its short columns; a provider field takes the whole row.
        let columns = if matches!(
            field,
            Field::AiModel | Field::AiEndpoint | Field::AiPath | Field::AiKey
        ) {
            width
        } else {
            NUMBER_WIDTH
        };
        let mut line = input::line(
            FieldText {
                text: input.text(),
                cursor: input.cursor(),
                width: columns,
                focused,
                mode,
            },
            theme,
        );
        if input.text().is_empty() {
            let hint = match field {
                Field::AiKey => choice(form, field),
                // The editor fields carry their own meaning.
                Field::ThemeName | Field::ThemeColors => String::new(),
                _ => form.inherited(field),
            };
            if !hint.is_empty() {
                line.spans.push(Span::styled(hint, theme.hint));
            }
        }
        return line;
    }
    let name = choice(form, field);
    if matches!(
        field,
        Field::ShortcutRecord
            | Field::ShortcutRemove
            | Field::ShortcutRestore
            | Field::ShortcutBack
            | Field::AiTest
            | Field::ThemeNew
            | Field::ThemeEdit
            | Field::ThemeDelete
            | Field::ThemeDone
            | Field::ThemeBack
    ) {
        let style = if focused {
            theme.dialog_focus
        } else {
            theme.dialog_option
        };
        return Line::from(Span::styled(theme.button(&name), style));
    }
    // Without a project, the scope has one value, so it is not a choice.
    if field == Field::Scope && !form.has_project() {
        return Line::raw(name);
    }
    choice_line(&name, focused, theme)
}

fn draw_field_row(frame: &mut Frame, area: Rect, form: &SettingsForm, field: Field, theme: &Theme) {
    draw_row(
        frame,
        area,
        (label(field), LABEL_WIDTH),
        form.focus == field,
        value(form, field, theme, value_width(area, LABEL_WIDTH)),
        theme,
    );
}

fn draw_command_list(
    frame: &mut Frame,
    pane: Rect,
    content: Rect,
    form: &SettingsForm,
    theme: &Theme,
    layout: &mut SettingsLayout,
) {
    let mode = Rect {
        y: pane.y + FIRST_FIELD_ROW,
        height: 1,
        ..pane
    };
    draw_field_row(frame, mode, form, Field::ShortcutMode, theme);
    layout.fields.push((Field::ShortcutMode, mode));
    let header = Rect {
        y: mode.y + 2,
        ..content
    };
    let key_width = content.width * 2 / 5;
    let command_width = content.width.saturating_sub(key_width + 2);
    let commands = form.shortcut_commands();
    let selected = commands
        .iter()
        .position(|(action, _)| *action == form.shortcuts.action)
        .unwrap_or(0);
    let heading = format!(
        "{} {}/{}",
        text::SHORTCUT_COMMANDS,
        selected + 1,
        commands.len()
    );
    frame.render_widget(
        Paragraph::new(heading).style(theme.panel_title),
        Rect {
            width: command_width,
            ..header
        },
    );
    let keys_x = header.x + command_width + 2;
    frame.render_widget(
        Paragraph::new(text::SHORTCUT_KEYS).style(theme.panel_title),
        Rect {
            x: keys_x,
            width: key_width,
            ..header
        },
    );
    let start = header.y + 1;
    let end = pane.bottom().saturating_sub(2 + NOTE_ROWS + 1);
    let rows = u16::try_from(usize::from(end.saturating_sub(start))).unwrap_or(0);
    let view = viewport_centered(selected, commands.len(), rows);
    let list = Rect {
        y: start,
        height: rows,
        ..content
    };
    layout.fields.push((Field::ShortcutCommands, list));
    for (offset, (action, keys)) in commands.iter().enumerate().skip(view.first).take(view.rows) {
        let row = Rect {
            y: start + u16::try_from(offset - view.first).unwrap_or(0),
            height: 1,
            ..content
        };
        layout.command_rows.push((*action, row));
        if *action == form.shortcuts.action {
            let style = if form.focus == Field::ShortcutCommands {
                theme.tree_selected
            } else {
                theme.tree_cursor
            };
            frame.buffer_mut().set_style(row, style);
            frame.render_widget(
                Paragraph::new(theme.icons.focus_mark).style(style),
                Rect {
                    x: row.x.saturating_sub(2),
                    width: 1,
                    ..row
                },
            );
        }
        let name = fit(text::command_text(*action), usize::from(command_width)).0;
        frame.render_widget(
            Paragraph::new(name),
            Rect {
                width: command_width,
                ..row
            },
        );
        let keys = if keys.is_empty() {
            text::SHORTCUT_UNASSIGNED.to_owned()
        } else {
            keys.iter()
                .map(|key| text::key_label(key))
                .collect::<Vec<_>>()
                .join(", ")
        };
        frame.render_widget(
            Paragraph::new(fit(&keys, usize::from(key_width)).0),
            Rect {
                x: keys_x,
                width: key_width,
                ..row
            },
        );
    }
}

fn draw_recording(
    frame: &mut Frame,
    content: Rect,
    button_row: u16,
    form: &SettingsForm,
    theme: &Theme,
    layout: &mut SettingsLayout,
) {
    let message = if form.shortcuts.mouse_confirm {
        text::SHORTCUT_RECORD_MOUSE
    } else {
        text::SHORTCUT_RECORD_KEYS
    };
    let sequence = text::key_label(&form.shortcuts.recorded());
    let lines = vec![
        Line::raw(text::action_text(form.shortcuts.action)),
        Line::raw(""),
        Line::styled(message, theme.hint),
        Line::raw(""),
        Line::styled(sequence, theme.dialog_focus),
    ];
    let body = Rect {
        y: content.y + 1,
        height: button_row.saturating_sub(content.y + 1 + NOTE_ROWS),
        ..content
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    if let Some(note) = &form.note {
        draw_note(frame, content, button_row, note, theme.error);
    }
    // During recording, only these buttons receive mouse clicks.
    layout.fields.clear();
    let buttons = buttons::dialog_items(
        [
            (Field::ShortcutOk, text::SHORTCUT_OK),
            (Field::ShortcutCancel, text::SHORTCUT_CANCEL),
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
}

/// Draws the list of the custom themes, with its buttons.
fn draw_theme_list(
    frame: &mut Frame,
    pane: Rect,
    content: Rect,
    button_row: u16,
    form: &SettingsForm,
    theme: &Theme,
    layout: &mut SettingsLayout,
) {
    let first = Rect {
        y: pane.y + FIRST_FIELD_ROW,
        height: 1,
        ..pane
    };
    draw_field_row(frame, first, form, Field::Theme, theme);
    layout.fields.push((Field::Theme, first));
    let header = Rect {
        y: first.y + 2,
        height: 1,
        ..content
    };
    frame.render_widget(
        Paragraph::new(text::FIELD_THEMES.to_uppercase()).style(theme.panel_title),
        header,
    );
    // Use only the rows the list needs, while reserving space for the buttons
    // and the note. Long lists scroll; an empty list still has its message.
    let note_top = button_row.saturating_sub(NOTE_ROWS);
    let list = form.theme_list();
    let available = note_top.saturating_sub(header.y + 1).saturating_sub(2);
    let rows = u16::try_from(list.len().max(1))
        .unwrap_or(u16::MAX)
        .min(available);
    let buttons_top = (header.y + 1).saturating_add(rows).saturating_add(1);
    let view = viewport_centered(form.theme_selected(), list.len(), rows);
    let area = Rect {
        y: header.y + 1,
        height: rows,
        ..content
    };
    layout.fields.push((Field::Themes, area));
    if list.is_empty() {
        frame.render_widget(Paragraph::new(text::THEME_NONE).style(theme.hint), area);
    }
    for (offset, item) in list.iter().enumerate().skip(view.first).take(view.rows) {
        let row = Rect {
            y: area.y + u16::try_from(offset - view.first).unwrap_or(0),
            height: 1,
            ..content
        };
        layout.theme_rows.push((item.id.clone(), row));
        if offset == form.theme_selected() {
            let style = if form.focus == Field::Themes {
                theme.tree_selected
            } else {
                theme.tree_cursor
            };
            frame.buffer_mut().set_style(row, style);
            frame.render_widget(
                Paragraph::new(theme.icons.focus_mark).style(style),
                Rect {
                    x: row.x.saturating_sub(2),
                    width: 1,
                    ..row
                },
            );
        }
        let origin = match item.scope {
            Scope::Project => text::VALUE_SCOPE_PROJECT,
            Scope::Global => text::VALUE_SCOPE_GLOBAL,
        };
        let line = fit(
            &format!(
                "{} · {} ({origin})",
                crate::grid::printable(&item.name),
                crate::grid::printable(&item.id)
            ),
            usize::from(content.width),
        )
        .0;
        frame.render_widget(Paragraph::new(line), row);
    }
    let buttons = buttons::dialog_items(
        [
            (Field::ThemeNew, text::BUTTON_THEME_NEW),
            (Field::ThemeEdit, text::BUTTON_THEME_EDIT),
            (Field::ThemeDelete, text::BUTTON_THEME_DELETE),
        ],
        form.focus,
        theme,
    );
    layout.fields.extend(buttons::draw(
        frame,
        Rect {
            y: buttons_top,
            height: 1,
            ..content
        },
        RowOptions::DIALOG,
        &buttons,
        theme,
    ));
}

/// Draws the theme editor: the name, the base, the colors, and its buttons.
fn draw_theme_editor(
    frame: &mut Frame,
    pane: Rect,
    content: Rect,
    button_row: u16,
    form: &SettingsForm,
    theme: &Theme,
    layout: &mut SettingsLayout,
) {
    let name = Rect {
        y: pane.y + FIRST_FIELD_ROW,
        height: 1,
        ..pane
    };
    draw_field_row(frame, name, form, Field::ThemeName, theme);
    layout.fields.push((Field::ThemeName, name));
    let mut next = name.y + 1;
    if form.theme_is_new() {
        let base = Rect {
            y: next,
            height: 1,
            ..pane
        };
        draw_field_row(frame, base, form, Field::ThemeBase, theme);
        layout.fields.push((Field::ThemeBase, base));
        next += 1;
    }
    let header = Rect {
        y: next + 1,
        height: 1,
        ..content
    };
    frame.render_widget(
        Paragraph::new(text::FIELD_THEMES.to_uppercase()).style(theme.panel_title),
        header,
    );
    let note_top = button_row.saturating_sub(NOTE_ROWS);
    let buttons_top = note_top.saturating_sub(2);
    let roles = form.theme_role_count();
    let rows = u16::try_from(usize::from(buttons_top.saturating_sub(header.y + 1))).unwrap_or(0);
    let view = viewport_centered(form.theme_cursor(), roles, rows);
    let area = Rect {
        y: header.y + 1,
        height: rows,
        ..content
    };
    layout.fields.push((Field::ThemeColors, area));
    for role in view.first..view.first.saturating_add(view.rows).min(roles) {
        let row = Rect {
            y: area.y + u16::try_from(role - view.first).unwrap_or(0),
            height: 1,
            ..pane
        };
        layout.color_rows.push((role, row));
        let Some((hex, color)) = form.theme_hex(role) else {
            continue;
        };
        let focused = form.focus == Field::ThemeColors && form.theme_cursor() == role;
        let mut line = input::line(
            FieldText {
                text: hex.text(),
                cursor: hex.cursor(),
                width: HEX_WIDTH,
                focused,
                mode: FieldMode::Text,
            },
            theme,
        );
        line.spans.push(Span::raw(" "));
        line.spans
            .push(Span::styled("  ", Style::new().bg(color).fg(color)));
        draw_row(
            frame,
            row,
            (SettingsForm::theme_role_label(role), LABEL_WIDTH),
            focused,
            line,
            theme,
        );
    }
    for (index, field) in [Field::ThemeDone, Field::ThemeBack].into_iter().enumerate() {
        let row = Rect {
            y: buttons_top + index as u16,
            height: 1,
            ..pane
        };
        draw_field_row(frame, row, form, field, theme);
        layout.fields.push((field, row));
    }
}

pub fn draw_settings(frame: &mut Frame, form: &SettingsForm, theme: &Theme) -> SettingsLayout {
    let width = modal_width(frame.area().width, WIDTH);
    let section = &SECTIONS[form.section];
    let shortcuts = section.fields.contains(&Field::ShortcutCommands);
    let themes = section.fields.contains(&Field::Themes);
    let height = if (shortcuts && !form.shortcuts.is_editing()) || themes {
        frame
            .area()
            .height
            .saturating_sub(4)
            .clamp(window_height(), 30)
    } else {
        window_height()
    };
    let titles: Vec<&str> = SECTIONS.iter().map(|section| section.title).collect();
    let modal = Modal::sized(width, height)
        .title(text::TITLE_SETTINGS)
        .heading_row(TITLE_ROW)
        .sidebar(SidebarSpec {
            titles: &titles,
            open: form.section,
            focused: form.focus == Field::Sections,
        })
        .open(frame, theme);
    let pane = modal.pane;
    let mut layout = SettingsLayout {
        sections: modal.sidebar_rows,
        fields: Vec::new(),
        command_rows: Vec::new(),
        theme_rows: Vec::new(),
        color_rows: Vec::new(),
    };

    let scope = Rect {
        y: pane.y + SCOPE_ROW,
        height: 1,
        ..pane
    };
    draw_field_row(frame, scope, form, Field::Scope, theme);
    if form.has_project() {
        layout.fields.push((Field::Scope, scope));
    }

    let content = modal.content;
    let title = if shortcuts && form.shortcuts.is_editing() {
        format!(
            "{} · {}",
            section.title.to_uppercase(),
            form.shortcuts.action.name()
        )
    } else if themes && form.theme_is_editing() {
        format!(
            "{} · {}",
            section.title.to_uppercase(),
            form.theme_editor_id()
        )
    } else {
        section.title.to_uppercase()
    };
    frame.render_widget(Paragraph::new(title).style(theme.panel_title), content);
    if form.shortcuts.is_recording() {
        draw_recording(frame, content, modal.button_row, form, theme, &mut layout);
        return layout;
    }
    if shortcuts && !form.shortcuts.is_editing() {
        draw_command_list(frame, pane, content, form, theme, &mut layout);
    } else if themes {
        if form.theme_is_editing() {
            draw_theme_editor(
                frame,
                pane,
                content,
                modal.button_row,
                form,
                theme,
                &mut layout,
            );
        } else {
            draw_theme_list(
                frame,
                pane,
                content,
                modal.button_row,
                form,
                theme,
                &mut layout,
            );
        }
    } else {
        let fields = if shortcuts {
            form.shortcut_stops()
        } else {
            section.fields
        };
        for (y, field) in (pane.y + FIRST_FIELD_ROW..).zip(fields) {
            let row = Rect {
                y,
                height: 1,
                ..pane
            };
            draw_field_row(frame, row, form, *field, theme);
            layout.fields.push((*field, row));
        }
    }

    let button_row = modal.button_row;
    if let Some(help) = section.help.filter(|_| !themes) {
        // One empty row after the fields. The help ends before the note.
        let top = pane.y + FIRST_FIELD_ROW + u16::try_from(section.fields.len()).unwrap_or(0) + 1;
        let help_area = Rect {
            y: top,
            height: button_row.saturating_sub(top + NOTE_ROWS),
            ..content
        };
        frame.render_widget(
            Paragraph::new(help)
                .style(theme.hint)
                .wrap(Wrap { trim: false }),
            help_area,
        );
    }
    if let Some(note) = &form.note {
        draw_note(frame, content, button_row, note, theme.error);
    } else if let Some(warning) = form.theme_contrast_warning() {
        draw_note(frame, content, button_row, &warning, theme.hint);
    } else if shortcuts {
        let action = form.shortcuts.action;
        let note = format!(
            "{} · {}",
            text::shortcut_region(action.region()),
            text::command_text(action)
        );
        draw_note(frame, content, button_row, &note, theme.hint);
    }
    let buttons = buttons::dialog_items(
        [
            (Field::Save, text::BUTTON_SAVE),
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
