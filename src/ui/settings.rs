//! The settings dialog.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::console::fit;
use super::theme::Theme;
use super::widgets::{
    BORDER_ROWS, FOOTER_ROWS, LEFT, NOTE_ROWS, RIGHT, SidebarModal, centered, choice_line,
    draw_buttons, draw_note, draw_row, draw_sidebar_rows, modal_width, value_width,
};
use super::{input_spans, text};
use crate::input::Action;
use crate::settings::{Field, SECTIONS, SettingsForm};
use crate::store::Scope;

pub(super) const WIDTH: u16 = 72;
const LABEL_WIDTH: u16 = 16;
/// The value columns of a number field.
const NUMBER_WIDTH: usize = 12;
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
        Field::Theme => form.theme().map_or_else(
            || form.inherited(field),
            |name| text::theme_name(name).to_owned(),
        ),
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
        field => form.shortcut_value(field),
    }
}

fn value<'a>(form: &SettingsForm, field: Field, theme: &Theme, width: usize) -> Line<'a> {
    let focused = form.focus == field;
    if let Some(input) = form.input(field) {
        let masked = field == Field::AiKey && !input.text().is_empty();
        let chars: Vec<char> = if masked {
            "*".repeat(input.text().chars().count().min(24))
                .chars()
                .collect()
        } else {
            input.text().chars().collect()
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
        let mut spans = input_spans(&chars, input.cursor(), columns, focused, theme).0;
        if chars.is_empty() {
            let hint = match field {
                Field::AiKey => choice(form, field),
                _ => form.inherited(field),
            };
            if !hint.is_empty() {
                spans.push(Span::styled(hint, theme.hint));
            }
        }
        return Line::from(spans);
    }
    let name = choice(form, field);
    if matches!(
        field,
        Field::ShortcutRecord
            | Field::ShortcutRemove
            | Field::ShortcutRestore
            | Field::ShortcutBack
            | Field::AiTest
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
    let rows = usize::from(end.saturating_sub(start));
    let first = selected
        .saturating_sub(rows / 2)
        .min(commands.len().saturating_sub(rows));
    let list = Rect {
        y: start,
        height: u16::try_from(rows).unwrap_or(0),
        ..content
    };
    layout.fields.push((Field::ShortcutCommands, list));
    for (offset, (action, keys)) in commands.iter().skip(first).take(rows).enumerate() {
        let row = Rect {
            y: start + u16::try_from(offset).unwrap_or(0),
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
    pane: Rect,
    content: Rect,
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
    let button_row = pane.bottom().saturating_sub(2);
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
    let buttons = [
        (Field::ShortcutOk, text::SHORTCUT_OK),
        (Field::ShortcutCancel, text::SHORTCUT_CANCEL),
    ];
    layout.fields.extend(draw_buttons(
        frame,
        content,
        button_row,
        (&buttons, form.focus),
        theme,
    ));
}

pub fn draw_settings(frame: &mut Frame, form: &SettingsForm, theme: &Theme) -> SettingsLayout {
    let width = modal_width(frame.area().width, WIDTH);
    let section = &SECTIONS[form.section];
    let shortcuts = section.fields.contains(&Field::ShortcutCommands);
    let height = if shortcuts && !form.shortcuts.is_editing() {
        frame
            .area()
            .height
            .saturating_sub(4)
            .clamp(window_height(), 30)
    } else {
        window_height()
    };
    let area = centered(frame.area(), width, height);
    let SidebarModal { sidebar, pane, .. } =
        SidebarModal::open(frame, area, text::TITLE_SETTINGS, theme);
    let titles: Vec<&str> = SECTIONS.iter().map(|section| section.title).collect();
    let open = (form.section, form.focus == Field::Sections);
    let mut layout = SettingsLayout {
        sections: draw_sidebar_rows(frame, sidebar, &titles, open, theme),
        fields: Vec::new(),
        command_rows: Vec::new(),
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

    let content = Rect {
        x: pane.x + LEFT,
        y: pane.y + TITLE_ROW,
        width: pane.width.saturating_sub(LEFT + RIGHT),
        height: 1,
    };
    let title = if shortcuts && form.shortcuts.is_editing() {
        format!(
            "{} · {}",
            section.title.to_uppercase(),
            form.shortcuts.action.name()
        )
    } else {
        section.title.to_uppercase()
    };
    frame.render_widget(Paragraph::new(title).style(theme.panel_title), content);
    if form.shortcuts.is_recording() {
        draw_recording(frame, pane, content, form, theme, &mut layout);
        return layout;
    }
    if shortcuts && !form.shortcuts.is_editing() {
        draw_command_list(frame, pane, content, form, theme, &mut layout);
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

    let button_row = pane.bottom().saturating_sub(2);
    if let Some(help) = section.help {
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
    } else if shortcuts {
        let action = form.shortcuts.action;
        let note = format!(
            "{} · {}",
            text::shortcut_region(action.region()),
            text::command_text(action)
        );
        draw_note(frame, content, button_row, &note, theme.hint);
    }
    let buttons = [
        (Field::Save, text::BUTTON_SAVE),
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
