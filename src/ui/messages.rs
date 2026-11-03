//! Compact command blocks. The view keeps only the visible styled rows.

use std::borrow::Cow;
use std::ops::Range;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;
use super::wrap::row_end;
use crate::command_log::{CommandState, Entry, History, ViewKey, limited};
use crate::console::MessageLevel;
use crate::grid::printable;
use crate::sql::{Dialect, Lexer, Token};

#[derive(Clone, Copy)]
enum Kind {
    Header,
    Sql,
    Summary {
        time: usize,
        state: CommandState,
        warning: bool,
    },
    Note(MessageLevel),
    Space,
}

struct Part<'a> {
    text: Cow<'a, str>,
    kind: Kind,
}

struct Layout<'a> {
    parts: Vec<Part<'a>>,
    dialect: Dialect,
}

impl<'a> Layout<'a> {
    fn new(entry: &'a Entry, width: usize, theme: &Theme) -> Self {
        let mut layout = Self {
            parts: Vec::new(),
            dialect: entry.dialect,
        };
        if entry.command.is_none() {
            layout.parts.push(Part {
                text: Cow::Borrowed(&entry.message.text),
                kind: Kind::Note(entry.message.level),
            });
            return layout;
        }
        let raw = &entry.message.text;
        let heading = raw[..entry.header_len]
            .trim_end_matches('\n')
            .strip_prefix("Command ");
        let label =
            heading.map_or_else(|| format!("#{}", entry.id), |heading| format!("#{heading}"));
        let rule = if theme.icons.done == "ok" { "-" } else { "─" };
        let mut heading = limited(format_args!("{rule}{rule} {label} "));
        heading.push_str(&rule.repeat(width.saturating_sub(heading.width())));
        layout.parts.push(Part {
            text: Cow::Owned(heading),
            kind: Kind::Header,
        });
        layout.space();
        layout.parts.push(Part {
            text: Cow::Borrowed(&raw[entry.header_len..entry.command_end]),
            kind: Kind::Sql,
        });
        layout.space();
        let state = entry
            .summary
            .as_ref()
            .map(|summary| summary.state)
            .or_else(|| entry.completion.as_ref().map(|completion| completion.state))
            .unwrap_or(CommandState::Running);
        let warning = entry.message.level == MessageLevel::Warning;
        let (mark, label) = match state {
            CommandState::Running => (theme.icons.running[0], "Running. Result unknown."),
            CommandState::Completed if warning => ("!", "Completed"),
            CommandState::Completed => (theme.icons.done, "Completed"),
            CommandState::Stopped => (theme.icons.stopped, "Stopped"),
            CommandState::TimedOut => (theme.icons.failed, "Timeout"),
            CommandState::Failed => (theme.icons.failed, "Error"),
            CommandState::Unknown => (theme.icons.failed, "Result unknown"),
        };
        let value = entry
            .summary
            .as_ref()
            .and_then(|summary| raw.get(summary.span.clone()))
            .map(|text| statement_text(entry, text))
            .or_else(|| {
                entry
                    .completion
                    .as_ref()
                    .filter(|_| state == CommandState::Failed)
                    .and_then(|completion| raw.get(completion.detail_start..completion.span.end))
                    .map(|text| text.strip_prefix("Error: ").unwrap_or(text))
            })
            .filter(|text| {
                !text.is_empty()
                    && *text != "done."
                    && !(state == CommandState::Stopped && *text == "stopped.")
            })
            .map(|text| {
                if state == CommandState::Completed && !text.contains(". ") {
                    text.trim_end_matches('.')
                } else {
                    text
                }
            });
        let mut summary = limited(format_args!(
            "{mark} {label}{}{}",
            if value.is_some() { " · " } else { "" },
            value.unwrap_or("")
        ));
        let time = summary.len();
        if let Some(completion) = &entry.completion {
            summary.push_str(&format!(
                " · {:.3} ms",
                completion.elapsed.as_secs_f64() * 1000.0
            ));
        }
        let kind = Kind::Summary {
            time,
            state,
            warning: warning && state == CommandState::Completed,
        };
        layout.parts.push(Part {
            text: Cow::Owned(summary),
            kind,
        });

        // Completion and row-count messages already appear in the summary.
        let mut omitted: Vec<Range<usize>> = entry
            .completion
            .iter()
            .map(|completion| completion.span.clone())
            .chain(entry.summary.iter().map(|summary| summary.span.clone()))
            .filter(|span| !span.is_empty())
            .collect();
        omitted.sort_by_key(|span| span.start);
        let mut at = entry.command_end;
        for span in omitted {
            layout.detail(entry, at..span.start.max(at));
            at = at.max(span.end);
        }
        layout.detail(entry, at..raw.len());
        layout.space();
        layout
    }

    fn space(&mut self) {
        self.parts.push(Part {
            text: Cow::Borrowed(""),
            kind: Kind::Space,
        });
    }

    fn detail(&mut self, entry: &'a Entry, range: Range<usize>) {
        if let Some(text) = entry.message.text.get(range) {
            let text = statement_text(entry, text.trim_matches('\n'));
            if !text.is_empty() {
                self.parts.push(Part {
                    text: Cow::Borrowed(text),
                    kind: Kind::Note(entry.message.level),
                });
            }
        }
    }

    fn count(&self, width: usize) -> usize {
        self.parts
            .iter()
            .map(|part| {
                let mut rows = 0;
                let mut next = Some((0, false));
                while let Some((at, _)) = next {
                    rows += 1;
                    next = row_end(&part.text, at, width).1;
                }
                rows
            })
            .sum()
    }

    fn rows(
        &self,
        width: usize,
        mut skip: usize,
        count: usize,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        let mut shown = Vec::new();
        for part in &self.parts {
            let mut next = Some((0, false));
            let mut lexer = Lexer::new(&part.text, self.dialect);
            let mut token = None;
            while let Some((at, _)) = next {
                let (end, after) = row_end(&part.text, at, width);
                if skip > 0 {
                    skip -= 1;
                } else {
                    let line = match part.kind {
                        Kind::Sql => sql_line(
                            &part.text,
                            at..end,
                            &mut lexer,
                            &mut token,
                            theme,
                            self.dialect,
                        ),
                        Kind::Summary {
                            time,
                            state,
                            warning,
                        } => {
                            let cut = time.clamp(at, end);
                            Line::from(vec![
                                Span::styled(
                                    printable(&part.text[at..cut]),
                                    if warning {
                                        theme.mark_neutral
                                    } else {
                                        state_style(state, theme)
                                    },
                                ),
                                Span::styled(printable(&part.text[cut..end]), theme.hint),
                            ])
                        }
                        Kind::Header => Line::styled(
                            printable(&part.text[at..end]),
                            theme.panel_title.add_modifier(Modifier::BOLD),
                        ),
                        Kind::Note(level) => {
                            Line::styled(printable(&part.text[at..end]), note_style(level, theme))
                        }
                        Kind::Space => Line::default(),
                    };
                    shown.push(line);
                    if shown.len() >= count {
                        return shown;
                    }
                }
                next = after;
            }
        }
        shown
    }
}

fn statement_text<'a>(entry: &Entry, text: &'a str) -> &'a str {
    entry
        .key
        .and_then(|key| text.strip_prefix(&format!("Statement {}: ", key.index + 1)))
        .unwrap_or(text)
}

fn note_style(level: MessageLevel, theme: &Theme) -> Style {
    match level {
        MessageLevel::Info => theme.hint,
        MessageLevel::Warning => theme.mark_neutral,
        MessageLevel::Error => theme.error,
    }
}

fn state_style(state: CommandState, theme: &Theme) -> Style {
    match state {
        CommandState::Completed => theme.mark_good,
        CommandState::Running => theme.run_active,
        CommandState::Stopped => theme.mark_neutral,
        CommandState::Failed | CommandState::TimedOut | CommandState::Unknown => theme.error,
    }
}

fn sql_line(
    text: &str,
    range: Range<usize>,
    lexer: &mut Lexer<'_>,
    current: &mut Option<Token>,
    theme: &Theme,
    dialect: Dialect,
) -> Line<'static> {
    let mut spans = Vec::new();
    let mut at = range.start;
    while at < range.end {
        while current.is_none_or(|token| token.end() <= at) {
            *current = lexer.next();
            if current.is_none() {
                break;
            }
        }
        let Some(token) = current.filter(|token| token.start() < range.end) else {
            spans.push(Span::raw(printable(&text[at..range.end])));
            break;
        };
        if at < token.start() {
            let end = token.start().min(range.end);
            spans.push(Span::raw(printable(&text[at..end])));
            at = end;
        }
        let end = token.end().min(range.end);
        if at < end {
            let style = theme
                .sql_token_style(token.kind, &text[token.start()..token.end()], dialect)
                .unwrap_or_default();
            spans.push(Span::styled(printable(&text[at..end]), style));
            at = end;
        }
    }
    Line::from(spans)
}

/// Builds styled rows without changing the stored command text.
pub fn viewport(
    history: &mut History,
    scroll: &mut usize,
    width: usize,
    height: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let key = ViewKey {
        width,
        compact: true,
        marks: [
            theme.icons.done.width(),
            theme.icons.failed.width(),
            theme.icons.stopped.width(),
        ],
    };
    history.visible(
        scroll,
        height,
        key,
        |entry| Layout::new(entry, width, theme).count(width),
        |entry, skip, count| Layout::new(entry, width, theme).rows(width, skip, count, theme),
        || Line::styled("[Older records removed.]", theme.hint),
    )
}
