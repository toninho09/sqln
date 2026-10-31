//! Password input. Rendering uses masked characters only.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::console::centered;
use super::theme::Theme;
use super::{input_spans, text};
use crate::connector::target::Secret;
use crate::grid::printable;
use crate::line_input::LineInput;

pub enum PasswordAction {
    None,
    Cancel,
    Submit { password: Secret, save: bool },
}

pub struct PasswordPrompt {
    pub title: String,
    pub destination: String,
    pub save_allowed: bool,
    pub save: bool,
    pub focus: usize,
    input: LineInput,
}

impl PasswordPrompt {
    pub fn new(name: &str, destination: String, save_allowed: bool) -> Self {
        Self {
            title: format!("{} · {}", text::TITLE_PASSWORD, printable(name)),
            destination,
            save_allowed,
            save: false,
            focus: 0,
            input: LineInput::default(),
        }
    }

    pub fn paste(&mut self, value: &str) {
        if self.focus == 0 {
            self.input.insert(value.lines().next().unwrap_or(""));
        }
    }

    pub fn on_key(&mut self, mut key: KeyEvent, vim: bool) -> PasswordAction {
        if self.focus != 0 {
            key.code = match key.code {
                KeyCode::Left => KeyCode::BackTab,
                KeyCode::Right => KeyCode::Tab,
                KeyCode::Char('h' | 'k') if vim => KeyCode::BackTab,
                KeyCode::Char('j' | 'l') if vim => KeyCode::Tab,
                code => code,
            };
        }
        match key.code {
            KeyCode::Esc => return PasswordAction::Cancel,
            KeyCode::Tab | KeyCode::Down | KeyCode::BackTab | KeyCode::Up => {
                let fields = if self.save_allowed {
                    &[0, 1, 2, 3][..]
                } else {
                    &[0, 2, 3][..]
                };
                let index = fields
                    .iter()
                    .position(|field| *field == self.focus)
                    .unwrap_or(0);
                let back = matches!(key.code, KeyCode::BackTab | KeyCode::Up);
                self.focus =
                    fields[(index + if back { fields.len() - 1 } else { 1 }) % fields.len()];
            }
            KeyCode::Enter if self.focus == 3 => return PasswordAction::Cancel,
            KeyCode::Enter | KeyCode::Char(' ') if self.focus == 1 && self.save_allowed => {
                self.save = !self.save
            }
            KeyCode::Enter => {
                return PasswordAction::Submit {
                    password: Secret::new(self.input.text()),
                    save: self.save && self.save_allowed,
                };
            }
            _ if self.focus == 0 => {
                self.input.on_key(key);
            }
            _ => {}
        }
        PasswordAction::None
    }

    pub fn click(&mut self, field: usize) -> PasswordAction {
        self.focus = field;
        if field == 1 || field >= 2 {
            self.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false)
        } else {
            PasswordAction::None
        }
    }
}

pub fn draw_password(
    frame: &mut Frame,
    prompt: &PasswordPrompt,
    theme: &Theme,
) -> Vec<(usize, Rect)> {
    let area = centered(frame.area(), 70, 9);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(format!(" {} ", prompt.title))
        .style(theme.dialog);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(printable(&prompt.destination)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let field = Rect::new(inner.x, inner.y + 2, inner.width, 1);
    let chars: Vec<char> = prompt.input.text().chars().map(|_| '•').collect();
    let mut spans = vec![Span::raw(format!("{}: ", text::FIELD_PASSWORD))];
    spans.extend(
        input_spans(
            &chars,
            prompt.input.cursor(),
            usize::from(inner.width).saturating_sub(10),
            prompt.focus == 0,
            theme,
        )
        .0,
    );
    frame.render_widget(Paragraph::new(Line::from(spans)), field);
    let mut layout = vec![(0, field)];
    let style = |field| {
        if prompt.focus == field {
            theme.dialog_focus
        } else {
            theme.dialog_option
        }
    };
    if prompt.save_allowed {
        let row = Rect::new(inner.x, inner.y + 3, inner.width, 1);
        let mark = if prompt.save { "[x]" } else { "[ ]" };
        frame.render_widget(
            Paragraph::new(format!("{mark} {}", text::SAVE_IN_VAULT)).style(style(1)),
            row,
        );
        layout.push((1, row));
    }
    frame.render_widget(
        Paragraph::new(text::AUTH_PASSWORD_HINT).style(theme.hint),
        Rect::new(inner.x, inner.y + 4, inner.width, 1),
    );
    let connect = format!("[{}]", text::BUTTON_CONNECT);
    let cancel = format!("[{}]", text::OPTION_CANCEL);
    let row = Rect::new(inner.x, inner.y + 6, inner.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(connect.clone(), style(2)),
            Span::raw("  "),
            Span::styled(cancel.clone(), style(3)),
        ])),
        row,
    );
    layout.push((2, Rect::new(row.x, row.y, connect.len() as u16, 1)));
    layout.push((
        3,
        Rect::new(
            row.x + connect.len() as u16 + 2,
            row.y,
            cancel.len() as u16,
            1,
        ),
    ));
    layout
}
