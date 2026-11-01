//! The status line: the focus, the connection, the state, and the use of resources.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::console::fit;
use super::text;
use super::theme::Theme;
use crate::connector::TransactionState;
use crate::console::{Phase, RunClock};
use crate::grid::printable;

const MIB: usize = 1024 * 1024;
const SEPARATOR: &str = " · ";
const GAP: &str = "  ";
/// The name of the connection keeps this width before a part of the row goes.
const MIN_NAME: usize = 8;

/// The place of the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusPanel {
    Editor,
    Normal,
    Insert,
    Visual,
    Results,
    Explorer,
    Filter,
    Table,
}

pub struct StatusData<'a> {
    pub panel: StatusPanel,
    pub connection: &'a str,
    pub schema: Option<&'a str>,
    /// The manual mode and the transaction. A table has neither.
    pub session: Option<(bool, TransactionState)>,
    pub phase: Phase,
    /// The time of the run in progress, or of the last run.
    pub run_time: Option<String>,
    pub pool: Option<(usize, usize)>,
    pub data: (usize, usize),
    /// The row and the column of the cursor, from 1.
    pub position: Option<(usize, usize)>,
}

fn panel_label(panel: StatusPanel, theme: &Theme) -> (&'static str, Style) {
    match panel {
        StatusPanel::Editor => (text::STATUS_EDITOR, theme.label_editor),
        StatusPanel::Normal => (text::STATUS_NORMAL, theme.label_editor),
        StatusPanel::Insert => (text::STATUS_INSERT, theme.label_insert),
        StatusPanel::Visual => (text::STATUS_VISUAL, theme.label_visual),
        StatusPanel::Results => (text::STATUS_RESULTS, theme.label_results),
        StatusPanel::Explorer => (text::STATUS_EXPLORER, theme.label_explorer),
        StatusPanel::Filter => (text::STATUS_FILTER, theme.label_insert),
        StatusPanel::Table => (text::STATUS_TABLE, theme.label_results),
    }
}

fn transaction_part(state: TransactionState, theme: &Theme) -> (&'static str, Style) {
    match state {
        TransactionState::None => (text::TRANSACTION_NONE, theme.status_detail),
        TransactionState::Open => (text::TRANSACTION_OPEN, theme.status_warning),
        TransactionState::Failed => (text::TRANSACTION_FAILED, theme.status_error),
        TransactionState::Unknown => (text::TRANSACTION_UNKNOWN, theme.status_error),
    }
}

fn phase_part(phase: Phase, theme: &Theme) -> (&'static str, Style) {
    match phase {
        Phase::Idle => (text::PHASE_IDLE, theme.status_detail),
        Phase::Waiting => (text::PHASE_WAITING, theme.status_warning),
        Phase::Running => (text::PHASE_RUNNING, theme.status_warning),
        Phase::Stopping => (text::PHASE_STOPPING, theme.status_warning),
    }
}

/// Returns the text of the time of a run, for the status line.
pub fn run_time(phase: Phase, run: &RunClock, now: Instant) -> Option<String> {
    if phase == Phase::Idle {
        run.last.map(text::last_run)
    } else {
        run.elapsed(now).map(text::elapsed)
    }
}

/// Lists the state. This part always stays on the row, because the user must see
/// the transaction and the execution.
fn state_parts(data: &StatusData, theme: &Theme) -> Vec<(String, Style)> {
    let mut parts = Vec::new();
    if let Some((manual, transaction)) = data.session {
        let mode = if manual {
            text::MODE_MANUAL
        } else {
            text::MODE_AUTO
        };
        parts.push((mode.to_owned(), theme.statusline));
        let (state, style) = transaction_part(transaction, theme);
        parts.push((state.to_owned(), style));
    }
    let (phase, style) = phase_part(data.phase, theme);
    let phase = data
        .run_time
        .as_ref()
        .map_or_else(|| phase.to_owned(), |time| format!("{phase} {time}"));
    parts.push((phase, style));
    parts
}

/// The parts of the row that can go, in the order of their removal.
struct Optional {
    position: Option<String>,
    data: Option<String>,
    pool: Option<String>,
    schema: Option<String>,
}

impl Optional {
    fn new(data: &StatusData) -> Self {
        Self {
            position: data.position.map(|(row, column)| format!("{row}:{column}")),
            data: Some(text::data_usage(
                data.data.0.div_ceil(MIB),
                data.data.1 / MIB,
            )),
            pool: data.pool.map(|(busy, limit)| text::pool_usage(busy, limit)),
            schema: data.schema.map(printable),
        }
    }

    fn usage(&self) -> Option<String> {
        let parts: Vec<&str> = [&self.pool, &self.data]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        (!parts.is_empty()).then(|| parts.join(SEPARATOR))
    }

    fn width(&self) -> usize {
        let part = |text: Option<&String>| text.map_or(0, |text| text.width() + GAP.len());
        part(self.position.as_ref()) + part(self.schema.as_ref()) + part(self.usage().as_ref())
    }

    /// Removes the next part. It returns false when no part remains.
    fn remove_next(&mut self) -> bool {
        for part in [
            &mut self.position,
            &mut self.data,
            &mut self.pool,
            &mut self.schema,
        ] {
            if part.take().is_some() {
                return true;
            }
        }
        false
    }
}

/// Shortens a name to a width, with a mark at the end of a short name.
fn short_name(name: &str, room: usize) -> String {
    if name.width() <= room {
        return name.to_owned();
    }
    let (start, _) = fit(name, room.saturating_sub(1));
    format!("{start}…")
}

/// One command of the hint bar: a key and its effect. A note has an empty key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HintItem {
    pub key: String,
    pub label: &'static str,
}

impl HintItem {
    pub fn of(parts: &[text::HintPart]) -> Vec<Self> {
        parts
            .iter()
            .map(|(key, label)| Self {
                key: (*key).to_owned(),
                label: *label,
            })
            .collect()
    }

    fn text(&self) -> String {
        if self.key.is_empty() {
            self.label.to_owned()
        } else {
            format!("{} {}", self.key, self.label)
        }
    }
}

/// Returns the text of a row of commands.
pub fn hint_text(items: &[HintItem]) -> String {
    let parts: Vec<String> = items.iter().map(HintItem::text).collect();
    parts.join(GAP)
}

/// Keeps the commands that fit a row, in their order. A command that does not fit stays
/// out whole, so the row shows no cut key, and a later short command can still fit.
pub fn fit_hints(items: &[HintItem], width: usize) -> Vec<HintItem> {
    let mut used = 0;
    let mut kept = Vec::new();
    for item in items {
        let gap = if kept.is_empty() { 0 } else { GAP.len() };
        let cost = gap + item.text().width();
        if used + cost <= width {
            used += cost;
            kept.push(item.clone());
        }
    }
    kept
}

/// Returns a row of commands. The style of the keys and the gap separate each command.
pub fn hint_line(items: &[HintItem], theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(GAP));
        }
        if !item.key.is_empty() {
            spans.push(Span::styled(item.key.clone(), theme.key));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(item.label, theme.hint));
    }
    Line::from(spans)
}

/// Draws the row. The state stays, and the name of the connection gives way to it.
pub fn draw_statusline(frame: &mut Frame, area: Rect, data: &StatusData, theme: &Theme) {
    frame.buffer_mut().set_style(area, theme.statusline);
    let (label, label_style) = panel_label(data.panel, theme);
    let label = format!(" {label} ");
    let state = state_parts(data, theme);
    let state_width: usize = state.iter().map(|(part, _)| part.width()).sum::<usize>()
        + SEPARATOR.width() * state.len().saturating_sub(1);
    let name = printable(data.connection);
    let width = usize::from(area.width);
    let fixed = label.width() + GAP.len() * 2 + state_width + 1;

    let mut optional = Optional::new(data);
    while fixed + name.width().min(MIN_NAME) + optional.width() > width && optional.remove_next() {}
    let room = width.saturating_sub(fixed + optional.width());

    let mut left = vec![
        Span::styled(label, label_style),
        Span::raw(GAP),
        Span::raw(short_name(&name, room)),
    ];
    if let Some(schema) = optional.schema.take_if(|schema| !schema.is_empty()) {
        left.push(Span::raw(GAP));
        left.push(Span::styled(schema, theme.status_detail));
    }
    left.push(Span::raw(GAP));
    for (index, (part, style)) in state.into_iter().enumerate() {
        if index > 0 {
            left.push(Span::styled(SEPARATOR, theme.status_detail));
        }
        left.push(Span::styled(part, style));
    }
    frame.render_widget(Paragraph::new(Line::from(left)), area);

    let mut right = Vec::new();
    if let Some(usage) = optional.usage() {
        right.push(Span::styled(usage, theme.status_detail));
    }
    if let Some(position) = optional.position {
        right.push(Span::raw(GAP));
        right.push(Span::raw(position));
    }
    right.push(Span::raw(" "));
    let used = u16::try_from(right.iter().map(Span::width).sum::<usize>()).unwrap_or(area.width);
    let start = area.right().saturating_sub(used).max(area.x);
    let place = Rect {
        x: start,
        width: area.right() - start,
        ..area
    };
    frame.render_widget(Paragraph::new(Line::from(right)), place);
}
