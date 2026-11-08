//! The bounded, memory-only command history of one console.

use std::collections::VecDeque;
use std::fmt::{self, Write};
use std::ops::{Index, Range};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::Sender;

use crate::console::{Message, MessageLevel};
use crate::exec::{ConnectionId, ExecEvent, StatementKey};
use crate::grid::printable;
use crate::sql::Dialect;
use crate::ui::wrap::row_end;

pub const HISTORY_BYTES: usize = 10 * 1024 * 1024;
const TEXT_BYTES: usize = HISTORY_BYTES - 1024;
const TRUNCATED: &str = "\n[Text truncated.]";
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// A formatter that never copies an unlimited command into an event.
pub struct LimitedText {
    text: String,
    limit: usize,
    truncated: bool,
}

impl LimitedText {
    pub fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
            truncated: false,
        }
    }

    pub fn finish(mut self) -> String {
        if self.truncated {
            self.text.push_str(TRUNCATED);
        }
        self.text.shrink_to_fit();
        self.text
    }
}

impl Write for LimitedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.truncated {
            return Err(fmt::Error);
        }
        let free = self.limit.saturating_sub(TRUNCATED.len() + self.text.len());
        let mut end = free.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&text[..end]);
        self.truncated = end != text.len();
        if self.truncated {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

pub fn limited(text: impl fmt::Display) -> String {
    let mut out = LimitedText::new(TEXT_BYTES);
    let _ = write!(out, "{text}");
    out.finish()
}

#[derive(Debug)]
pub enum LogEvent {
    Started {
        id: u64,
        key: Option<StatementKey>,
        user: bool,
        system: bool,
        header_len: usize,
        dialect: Dialect,
        text: String,
    },
    Finished {
        id: u64,
        level: MessageLevel,
        text: String,
        elapsed: Duration,
    },
}

/// The destination is fixed before an operation starts. Clones keep that destination.
#[derive(Clone)]
pub struct TraceContext {
    pub target: u64,
    pub connection: ConnectionId,
    pub name: String,
    pub shared: bool,
    pub key: Option<StatementKey>,
    pub user: bool,
    pub dialect: Dialect,
    events: Sender<ExecEvent>,
}

impl TraceContext {
    pub fn new(
        target: u64,
        connection: ConnectionId,
        name: String,
        shared: bool,
        events: Sender<ExecEvent>,
    ) -> Self {
        Self {
            target,
            connection,
            name,
            shared,
            key: None,
            user: false,
            dialect: Dialect::Postgres,
            events,
        }
    }

    fn started(&self, command: impl fmt::Display) -> (u64, ExecEvent) {
        let id = next_id();
        let origin = if self.shared {
            "Shared"
        } else if self.user {
            "User"
        } else {
            "Internal"
        };
        let header = limited(format_args!("Command {id} · {} · {origin}\n", self.name));
        let text = limited(format_args!("{header}{command}"));
        let header_len = header.len().min(text.len());
        (
            id,
            ExecEvent::CommandLog {
                target: self.target,
                event: LogEvent::Started {
                    id,
                    key: self.key,
                    user: self.user,
                    system: self.shared || !self.user,
                    header_len,
                    dialect: self.dialect,
                    text,
                },
            },
        )
    }

    pub fn start_blocking(&self, command: impl fmt::Display) -> CommandSpan {
        let (id, event) = self.started(command);
        let _ = self.events.blocking_send(event);
        CommandSpan {
            id,
            trace: self.clone(),
            started: Instant::now(),
            finished: false,
        }
    }

    pub fn internal(&self) -> Self {
        let mut trace = self.clone();
        trace.user = false;
        trace
    }

    pub fn start<C: fmt::Display>(
        &self,
        command: C,
    ) -> impl Future<Output = CommandSpan> + Send + use<C> {
        let (id, event) = self.started(command);
        let trace = self.clone();
        async move {
            let _ = trace.events.send(event).await;
            CommandSpan {
                id,
                trace,
                started: Instant::now(),
                finished: false,
            }
        }
    }
}

/// A dropped database future leaves an explicit unknown result, not a false success.
pub struct CommandSpan {
    id: u64,
    trace: TraceContext,
    started: Instant,
    finished: bool,
}

impl CommandSpan {
    fn result_event<T, E: fmt::Display>(&mut self, result: &Result<T, E>) -> ExecEvent {
        let elapsed = self.started.elapsed();
        self.finished = true;
        let (level, text) = match result {
            Ok(_) => (MessageLevel::Info, "Completed.".to_owned()),
            Err(error) => (MessageLevel::Error, limited(format_args!("Error: {error}"))),
        };
        ExecEvent::CommandLog {
            target: self.trace.target,
            event: LogEvent::Finished {
                id: self.id,
                level,
                text,
                elapsed,
            },
        }
    }

    fn finish_blocking<T, E: fmt::Display>(mut self, result: &Result<T, E>) {
        let event = self.result_event(result);
        let _ = self.trace.events.blocking_send(event);
    }

    pub fn finish<T, E: fmt::Display>(
        mut self,
        result: &Result<T, E>,
    ) -> impl Future<Output = ()> + Send + use<T, E> {
        let event = self.result_event(result);
        async move {
            let _ = self.trace.events.send(event).await;
        }
    }
}

impl Drop for CommandSpan {
    fn drop(&mut self) {
        if !self.finished {
            // Never block the runtime while it drops a canceled future. An unfinished
            // entry already says that the result is unknown.
            let _ = self.trace.events.try_send(ExecEvent::CommandLog {
                target: self.trace.target,
                event: LogEvent::Finished {
                    id: self.id,
                    level: MessageLevel::Info,
                    text: "Interrupted. Result unknown.".to_owned(),
                    elapsed: self.started.elapsed(),
                },
            });
        }
    }
}

pub fn start<C: fmt::Display>(
    trace: &Option<TraceContext>,
    command: C,
) -> impl Future<Output = Option<CommandSpan>> + Send + use<C> {
    let pending = trace.as_ref().map(|trace| trace.start(command));
    async move {
        match pending {
            Some(pending) => Some(pending.await),
            None => None,
        }
    }
}

pub fn finish<T, E: fmt::Display>(
    span: Option<CommandSpan>,
    result: &Result<T, E>,
) -> impl Future<Output = ()> + Send + use<T, E> {
    let pending = span.map(|span| span.finish(result));
    async move {
        if let Some(pending) = pending {
            pending.await;
        }
    }
}

pub fn observe<T, E: fmt::Display, F: Future<Output = Result<T, E>>>(
    trace: Option<TraceContext>,
    command: impl fmt::Display,
    future: F,
) -> impl Future<Output = Result<T, E>> {
    let pending = start(&trace, command);
    async move {
        let span = pending.await;
        let result = future.await;
        finish(span, &result).await;
        result
    }
}

pub fn observe_blocking<T, E: fmt::Display>(
    trace: &Option<TraceContext>,
    command: impl fmt::Display,
    run: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let span = trace.as_ref().map(|trace| trace.start_blocking(command));
    let result = run();
    if let Some(span) = span {
        span.finish_blocking(&result);
    }
    result
}

struct TracedCanceller {
    inner: std::sync::Arc<dyn crate::connector::Canceller>,
    trace: Option<TraceContext>,
}

#[async_trait::async_trait]
impl crate::connector::Canceller for TracedCanceller {
    async fn cancel(&self) -> Result<(), String> {
        self.inner.cancel_traced(self.trace.clone()).await
    }
}

pub fn canceller(
    inner: std::sync::Arc<dyn crate::connector::Canceller>,
    trace: &Option<TraceContext>,
) -> std::sync::Arc<dyn crate::connector::Canceller> {
    if trace.is_none() {
        return inner;
    }
    std::sync::Arc::new(TracedCanceller {
        inner,
        trace: trace.as_ref().map(TraceContext::internal),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandState {
    Running,
    Completed,
    Failed,
    Stopped,
    TimedOut,
    Unknown,
}

#[derive(Clone, Copy)]
struct CommandInfo {
    user: bool,
    system: bool,
    header_len: usize,
    dialect: Dialect,
}

#[derive(Debug)]
pub(crate) struct Completion {
    pub span: Range<usize>,
    pub detail_start: usize,
    pub elapsed: Duration,
    pub state: CommandState,
}

#[derive(Debug)]
pub(crate) struct Summary {
    pub span: Range<usize>,
    pub state: CommandState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewKey {
    pub width: usize,
    pub compact: bool,
    pub marks: [usize; 3],
}

#[derive(Debug)]
pub(crate) struct Entry {
    pub id: u64,
    pub key: Option<StatementKey>,
    pub message: Message,
    pub command: Option<bool>,
    system: bool,
    pub pending: bool,
    pub header_len: usize,
    pub command_end: usize,
    pub dialect: Dialect,
    pub completion: Option<Completion>,
    pub summary: Option<Summary>,
    layout: Option<ViewKey>,
    rows: usize,
}

#[derive(Debug)]
pub struct History {
    pub id: u64,
    entries: VecDeque<Entry>,
    bytes: usize,
    limit: usize,
    removed: bool,
    removed_key: Option<(u64, usize)>,
    anchor: Option<(u64, usize)>,
    last_scroll: usize,
    last_end: usize,
    following: bool,
    show_system_commands: bool,
}

impl Default for History {
    fn default() -> Self {
        Self::with_limit(HISTORY_BYTES)
    }
}

impl History {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            id: next_id(),
            entries: VecDeque::new(),
            bytes: 0,
            limit,
            removed: false,
            removed_key: None,
            anchor: None,
            last_scroll: 0,
            last_end: 0,
            following: true,
            show_system_commands: crate::store::config::DEFAULT_SHOW_SYSTEM_COMMANDS,
        }
    }

    /// Changes only the view. Hidden records keep their budget and their results.
    pub fn set_show_system_commands(&mut self, show: bool) -> bool {
        if self.show_system_commands == show {
            return false;
        }
        self.show_system_commands = show;
        self.anchor = self.anchor.filter(|(id, _)| {
            self.entries
                .iter()
                .any(|entry| entry.id == *id && (show || !entry.system))
        });
        true
    }

    /// Bounds each event, so a wheel burst at an edge cannot delay the reverse direction.
    pub fn scroll_by(&self, scroll: &mut usize, rows: isize) {
        *scroll = (*scroll)
            .min(self.last_end)
            .saturating_add_signed(rows)
            .min(self.last_end);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Message> {
        self.entries.iter().map(|entry| &entry.message)
    }
    pub(crate) fn agent_messages(&self) -> impl Iterator<Item = &Message> {
        self.entries
            .iter()
            .filter(|entry| self.show_system_commands || !entry.system)
            .map(|entry| &entry.message)
    }

    pub fn bytes(&self) -> usize {
        size_of::<Self>() + self.bytes + self.entries.capacity() * size_of::<Entry>()
    }

    fn bound(&mut self) {
        while size_of::<Self>() + self.bytes + self.entries.len() * size_of::<Entry>() > self.limit
        {
            let Some(entry) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= entry.message.text.capacity();
            if let Some(key) = entry.key {
                self.removed_key =
                    Some(self.removed_key.map_or((key.execution, key.index), |old| {
                        old.max((key.execution, key.index))
                    }));
            }
            self.removed = true;
        }
        if self.bytes() > self.limit {
            self.entries.shrink_to_fit();
        }
    }

    pub fn push(&mut self, level: MessageLevel, text: &str, key: Option<StatementKey>) {
        let mut out = LimitedText::new(
            self.limit
                .saturating_sub(4 * size_of::<Entry>() + size_of::<Self>()),
        );
        let _ = out.write_str(text);
        self.insert(next_id(), key, level, out.finish(), None);
    }

    fn insert(
        &mut self,
        id: u64,
        key: Option<StatementKey>,
        level: MessageLevel,
        text: String,
        info: Option<CommandInfo>,
    ) {
        let free = self
            .limit
            .saturating_sub(4 * size_of::<Entry>() + size_of::<Self>());
        let text = if text.capacity() > free {
            let mut out = LimitedText::new(free);
            let _ = out.write_str(&text);
            out.finish()
        } else {
            text
        };
        self.bytes += text.capacity();
        let command_end = text.len();
        let mut header_len = info.map_or(0, |info| info.header_len).min(command_end);
        while !text.is_char_boundary(header_len) {
            header_len -= 1;
        }
        self.entries.push_back(Entry {
            id,
            key,
            message: Message { level, text },
            command: info.map(|info| info.user),
            system: info.is_some_and(|info| info.system),
            pending: info.is_some(),
            header_len,
            command_end,
            dialect: info.map_or(Dialect::Postgres, |info| info.dialect),
            completion: None,
            summary: None,
            layout: None,
            rows: 0,
        });
        self.bound();
    }

    fn append(&mut self, index: usize, level: MessageLevel, text: &str) -> Range<usize> {
        let entry = &mut self.entries[index];
        self.bytes -= entry.message.text.capacity();
        let free = self
            .limit
            .saturating_sub(4 * size_of::<Entry>() + size_of::<Self>());
        let mut note = LimitedText::new(free / 2);
        let _ = note.write_str(text);
        let note = note.finish();
        let mut prefix = LimitedText::new(free.saturating_sub(note.len() + 1));
        let _ = prefix.write_str(&entry.message.text);
        let preserved = prefix.text.len();
        entry.message.text = prefix.finish();
        entry.command_end = entry.command_end.min(entry.message.text.len());
        entry.header_len = entry.header_len.min(entry.command_end);
        if let Some(completion) = &mut entry.completion
            && completion.span.end > preserved
        {
            completion.span = 0..0;
        }
        if let Some(summary) = &mut entry.summary
            && summary.span.end > preserved
        {
            summary.span = 0..0;
        }
        entry.message.text.push('\n');
        let start = entry.message.text.len();
        entry.message.text.push_str(&note);
        let span = start..entry.message.text.len();
        entry.message.text.shrink_to_fit();
        if level == MessageLevel::Error
            || (level == MessageLevel::Warning && entry.message.level != MessageLevel::Error)
        {
            entry.message.level = level;
        }
        entry.layout = None;
        self.bytes += entry.message.text.capacity();
        self.bound();
        span
    }

    pub fn note(&mut self, level: MessageLevel, text: &str, key: Option<StatementKey>) {
        self.note_inner(level, text, key, None);
    }

    pub fn summarize(
        &mut self,
        level: MessageLevel,
        text: &str,
        key: Option<StatementKey>,
        state: CommandState,
    ) {
        self.note_inner(level, text, key, Some(state));
    }

    fn note_inner(
        &mut self,
        level: MessageLevel,
        text: &str,
        key: Option<StatementKey>,
        state: Option<CommandState>,
    ) {
        let index = key.and_then(|key| {
            self.entries
                .iter()
                .rposition(|entry| entry.key == Some(key) && entry.command == Some(true))
                .or_else(|| {
                    self.entries.iter().rposition(|entry| {
                        entry.key == Some(key) && entry.command.is_some() && !entry.system
                    })
                })
        });
        if let Some(index) = index {
            let id = self.entries[index].id;
            let span = self.append(index, level, text);
            if let Some(state) = state
                && let Some(entry) = self.entries.iter_mut().rev().find(|entry| entry.id == id)
            {
                entry.summary = Some(Summary { span, state });
            }
        } else if !key.is_some_and(|key| {
            self.removed_key
                .is_some_and(|old| (key.execution, key.index) <= old)
        }) {
            self.push(level, text, key);
        }
    }

    pub fn on_event(&mut self, event: LogEvent) {
        match event {
            LogEvent::Started {
                id,
                key,
                user,
                system,
                header_len,
                dialect,
                text,
            } => {
                let note = if user && key.is_some() {
                    self.entries
                        .iter()
                        .rposition(|entry| entry.command.is_none() && entry.key == key)
                        .and_then(|index| self.entries.remove(index))
                } else {
                    None
                };
                if let Some(note) = &note {
                    self.bytes -= note.message.text.capacity();
                }
                self.insert(
                    id,
                    key,
                    MessageLevel::Info,
                    text,
                    Some(CommandInfo {
                        user,
                        system,
                        header_len,
                        dialect,
                    }),
                );
                if let Some(note) = note
                    && let Some(index) = self.entries.iter().rposition(|entry| entry.id == id)
                {
                    self.append(index, note.message.level, &note.message.text);
                }
            }
            LogEvent::Finished {
                id,
                level,
                text,
                elapsed,
            } => {
                if let Some(index) = self.entries.iter().rposition(|entry| entry.id == id) {
                    self.entries[index].pending = false;
                    let prefix = format!("Time: {:.3} ms. ", elapsed.as_secs_f64() * 1000.0);
                    let span = self.append(index, level, &limited(format_args!("{prefix}{text}")));
                    if let Some(entry) = self.entries.iter_mut().rev().find(|entry| entry.id == id)
                    {
                        let state = if level == MessageLevel::Error {
                            CommandState::Failed
                        } else if text == "Completed." {
                            CommandState::Completed
                        } else {
                            CommandState::Unknown
                        };
                        entry.completion = Some(Completion {
                            detail_start: span.start + prefix.len(),
                            span,
                            elapsed,
                            state,
                        });
                    }
                }
            }
        }
    }

    /// Returns only visible rows. Hidden commands keep their records.
    pub fn viewport(&mut self, scroll: &mut usize, width: usize, height: usize) -> Vec<Message> {
        let width = width.max(1);
        self.visible(
            scroll,
            height,
            ViewKey {
                width,
                compact: false,
                marks: [0; 3],
            },
            |entry| row_count(&entry.message.text, width) + usize::from(entry.pending),
            |entry, skip, count| raw_rows(entry, width, skip, count),
            || Message::info("[Older records removed.]"),
        )
    }

    pub(crate) fn visible<R>(
        &mut self,
        scroll: &mut usize,
        height: usize,
        key: ViewKey,
        count: impl Fn(&Entry) -> usize,
        rows: impl Fn(&Entry, usize, usize) -> Vec<R>,
        removed: impl FnOnce() -> R,
    ) -> Vec<R> {
        if height == 0 {
            return Vec::new();
        }
        let show_system = self.show_system_commands;
        let mut total = usize::from(self.removed);
        for entry in self
            .entries
            .iter_mut()
            .filter(|entry| show_system || !entry.system)
        {
            if entry.layout != Some(key) {
                entry.rows = count(entry);
                entry.layout = Some(key);
            }
            total += entry.rows;
        }
        let end = total.saturating_sub(height);
        if *scroll != self.last_scroll {
            self.following = self.last_end > 0 && *scroll >= self.last_end;
            self.anchor = None;
        }
        if self.following {
            *scroll = end;
        } else if let Some((id, row)) = self.anchor {
            let mut before = usize::from(self.removed);
            *scroll = 0;
            for entry in self
                .entries
                .iter()
                .filter(|entry| show_system || !entry.system)
            {
                if entry.id == id {
                    *scroll = before + row.min(entry.rows.saturating_sub(1));
                    break;
                }
                before += entry.rows;
            }
        }
        *scroll = (*scroll).min(end);
        self.following |= *scroll == end;
        self.last_scroll = *scroll;
        self.last_end = end;
        self.anchor = None;
        let mut skip = *scroll;
        let mut shown = Vec::new();
        if self.removed {
            if skip == 0 {
                shown.push(removed());
            } else {
                skip -= 1;
            }
        }
        for entry in self
            .entries
            .iter()
            .filter(|entry| show_system || !entry.system)
        {
            if shown.len() >= height {
                break;
            }
            if skip >= entry.rows {
                skip -= entry.rows;
                continue;
            }
            if self.anchor.is_none() {
                self.anchor = Some((entry.id, skip));
            }
            shown.extend(rows(entry, skip, height - shown.len()));
            skip = 0;
        }
        shown
    }
}

fn raw_rows(entry: &Entry, width: usize, skip: usize, count: usize) -> Vec<Message> {
    let mut shown = Vec::new();
    let mut next = Some((0, false));
    let mut row = 0;
    while let Some((at, _)) = next {
        let (last, after) = row_end(&entry.message.text, at, width);
        if row >= skip {
            shown.push(Message {
                level: entry.message.level,
                text: printable(&entry.message.text[at..last]),
            });
            if shown.len() >= count {
                return shown;
            }
        }
        row += 1;
        next = after;
    }
    if entry.pending && row >= skip && shown.len() < count {
        shown.push(Message::info("Running. Result unknown."));
    }
    shown
}

fn row_count(text: &str, width: usize) -> usize {
    let mut next = Some((0, false));
    let mut count = 0;
    while let Some((at, _)) = next {
        next = row_end(text, at, width).1;
        count += 1;
    }
    count
}

impl Index<usize> for History {
    type Output = Message;
    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index].message
    }
}
