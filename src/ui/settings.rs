//! The settings dialog.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::console::centered;
use super::theme::Theme;
use super::{input_spans, text};
use crate::grid::printable;
use crate::settings::{Field, SettingsForm};
use crate::store::Scope;

const LABEL_WIDTH: usize = 14;

fn row(frame: &mut Frame, inner: Rect, y: u16, label: &str, value: Vec<Span<'_>>) {
    let mut spans = vec![Span::raw(format!("{label:<LABEL_WIDTH$}"))];
    spans.extend(value);
    frame.render_widget(Paragraph::new(Line::from(spans)), Rect::new(inner.x, inner.y + y, inner.width, 1));
}

pub fn draw_settings(frame: &mut Frame, form: &SettingsForm, theme: &Theme) {
    let area = centered(frame.area(), 66, 12);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(format!(" {} ", text::TITLE_SETTINGS)).style(theme.dialog);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let choice = |field: Field, value: String| {
        let style = if form.focus == field { theme.dialog_focus } else { theme.dialog_option };
        vec![Span::styled(format!("[{value}]"), style)]
    };

    let scope = match form.scope {
        Scope::Global => text::VALUE_SCOPE_GLOBAL,
        Scope::Project => text::VALUE_SCOPE_PROJECT,
    };
    let scope = if form.has_project() { choice(Field::Scope, scope.to_owned()) } else { vec![Span::raw(scope)] };
    row(frame, inner, 0, text::FIELD_SCOPE, scope);
    let keyboard = form
        .keyboard()
        .map_or_else(|| form.inherited(Field::Keyboard), text::keyboard_name);
    row(frame, inner, 1, text::FIELD_KEYBOARD, choice(Field::Keyboard, keyboard));

    let numbers = [
        (Field::PageSize, text::FIELD_PAGE_SIZE),
        (Field::RowCap, text::FIELD_ROW_CAP),
        (Field::Budget, text::FIELD_BUDGET),
    ];
    for (y, (field, label)) in (2u16..).zip(numbers) {
        let Some(input) = form.input(field) else { continue };
        let chars: Vec<char> = input.text().chars().collect();
        let mut spans = input_spans(&chars, input.cursor(), 12, form.focus == field, theme).0;
        if input.text().is_empty() {
            spans.push(Span::styled(format!("  {}", form.inherited(field)), theme.hint));
        }
        row(frame, inner, y, label, spans);
    }

    let note = form.note.as_deref().map_or_else(String::new, printable);
    frame.render_widget(
        Paragraph::new(note).style(theme.status),
        Rect::new(inner.x, inner.y + 6, inner.width, 1),
    );
    let style = |field| if form.focus == field { theme.dialog_focus } else { theme.dialog_option };
    let buttons = Line::from(vec![
        Span::styled(format!("[{}]", text::BUTTON_SAVE), style(Field::Save)),
        Span::raw("  "),
        Span::styled(format!("[{}]", text::OPTION_CANCEL), style(Field::Cancel)),
    ]);
    frame.render_widget(Paragraph::new(buttons), Rect::new(inner.x, inner.y + 8, inner.width, 1));
}
