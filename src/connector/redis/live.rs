//! Commands that do not end: Pub/Sub, `MONITOR`, `SYNC`, and `PSYNC`. Each command uses
//! its own connection, so a stop closes that connection and the session stays clean.
//! The connection uses RESP2, because the RDB file of a synchronization is not RESP3.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{RedisCommand, open_stream, set_columns};
use crate::connector::ExecError;
use crate::connector::target::NetworkTarget;
use crate::exec::{Flow, ResultSink};
use crate::grid::{Row, RowWriter, Value};
use crate::ui::text;

/// The longest line of a reply, without its value.
const MAX_LINE: usize = 64 * 1024;
/// The largest value, as the `proto-max-bulk-len` default of the server.
const MAX_BULK: usize = 512 * 1024 * 1024;
/// The deepest list inside a list.
const MAX_DEPTH: usize = 32;
/// The size of each read from the network.
const READ_SIZE: usize = 16 * 1024;
/// A replica that sends no acknowledgment for `repl-timeout` loses its connection.
const ACK_EVERY: Duration = Duration::from_secs(1);

/// One RESP2 reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Simple(Vec<u8>),
    Error(String),
    Int(i64),
    Bulk(Option<Vec<u8>>),
    Array(Option<Vec<Frame>>),
}

fn bad_reply() -> String {
    text::REDIS_LIVE_BAD.to_owned()
}

/// Returns the line at `at`, without its end, and the position after the end.
fn line(buf: &[u8], at: usize) -> Option<(&[u8], usize)> {
    let end = buf.get(at..)?.windows(2).position(|pair| pair == b"\r\n")?;
    Some((&buf[at..at + end], at + end + 2))
}

fn number(bytes: &[u8]) -> Result<i64, String> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(bad_reply)
}

/// Reads one reply at the start of `buf`. It returns the reply and its size, or `None`
/// when the reply is not complete.
pub fn parse(buf: &[u8]) -> Result<Option<(Frame, usize)>, String> {
    parse_at(buf, 0, 0)
}

fn parse_at(buf: &[u8], at: usize, depth: usize) -> Result<Option<(Frame, usize)>, String> {
    if depth > MAX_DEPTH {
        return Err(bad_reply());
    }
    let Some(&kind) = buf.get(at) else {
        return Ok(None);
    };
    let Some((body, next)) = line(buf, at + 1) else {
        return if buf.len() - at > MAX_LINE {
            Err(bad_reply())
        } else {
            Ok(None)
        };
    };
    let frame = match kind {
        b'+' => (Frame::Simple(body.to_vec()), next),
        b'-' => (
            Frame::Error(String::from_utf8_lossy(body).into_owned()),
            next,
        ),
        b':' => (Frame::Int(number(body)?), next),
        b'$' => {
            let Ok(size) = usize::try_from(number(body)?) else {
                return Ok(Some((Frame::Bulk(None), next)));
            };
            if size > MAX_BULK {
                return Err(bad_reply());
            }
            let end = next + size;
            if buf.len() < end + 2 {
                return Ok(None);
            }
            if &buf[end..end + 2] != b"\r\n" {
                return Err(bad_reply());
            }
            (Frame::Bulk(Some(buf[next..end].to_vec())), end + 2)
        }
        b'*' => {
            let Ok(count) = usize::try_from(number(body)?) else {
                return Ok(Some((Frame::Array(None), next)));
            };
            let mut items = Vec::with_capacity(count.min(1024));
            let mut at = next;
            for _ in 0..count {
                let Some((item, end)) = parse_at(buf, at, depth + 1)? else {
                    return Ok(None);
                };
                items.push(item);
                at = end;
            }
            (Frame::Array(Some(items)), at)
        }
        _ => return Err(bad_reply()),
    };
    Ok(Some(frame))
}

/// Reads replies from the network.
pub struct Reader<R> {
    inner: R,
    buf: Vec<u8>,
    start: usize,
}

impl<R: AsyncRead + Unpin> Reader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buf: Vec::new(),
            start: 0,
        }
    }

    /// Returns true when a reply already waits in memory, so the next read does not wait.
    pub fn has_buffered(&self) -> bool {
        self.start < self.buf.len()
    }

    /// Reads more bytes. It is safe to cancel: the bytes go to the buffer only after the
    /// read.
    async fn fill(&mut self) -> Result<(), ExecError> {
        if self.start > 0 {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.reserve(READ_SIZE);
        let read = self
            .inner
            .read_buf(&mut self.buf)
            .await
            .map_err(|error| ExecError::new(error.to_string()))?;
        if read == 0 {
            return Err(ExecError::new(text::REDIS_LIVE_CLOSED));
        }
        Ok(())
    }

    /// Returns the next reply and its size in bytes.
    pub async fn frame(&mut self) -> Result<(Frame, usize), ExecError> {
        loop {
            if let Some((frame, size)) = parse(&self.buf[self.start..]).map_err(ExecError::new)? {
                self.start += size;
                return Ok((frame, size));
            }
            self.fill().await?;
        }
    }

    /// Reads the RDB file of a full synchronization and drops its bytes. It returns the
    /// size of the file. The server sends empty lines while it makes the file.
    pub async fn skip_rdb(&mut self) -> Result<usize, ExecError> {
        let size = loop {
            let pending = &self.buf[self.start..];
            if pending.first() == Some(&b'\n') {
                self.start += 1;
                continue;
            }
            if let Some((header, next)) = line(pending, 0) {
                let size = header
                    .strip_prefix(b"$")
                    .ok_or_else(bad_reply)
                    .and_then(number)
                    .and_then(|size| usize::try_from(size).map_err(|_| bad_reply()))
                    .map_err(ExecError::new)?;
                self.start += next;
                break size;
            }
            if pending.len() > MAX_LINE {
                return Err(ExecError::new(bad_reply()));
            }
            self.fill().await?;
        };
        let mut left = size;
        loop {
            let take = left.min(self.buf.len() - self.start);
            self.start += take;
            left -= take;
            if left == 0 {
                return Ok(size);
            }
            self.fill().await?;
        }
    }
}

/// Sends one command as a RESP2 list of values.
async fn send(write: &mut (impl AsyncWrite + Unpin), words: &[&[u8]]) -> Result<(), ExecError> {
    let mut packed = format!("*{}\r\n", words.len()).into_bytes();
    for word in words {
        packed.extend_from_slice(format!("${}\r\n", word.len()).as_bytes());
        packed.extend_from_slice(word);
        packed.extend_from_slice(b"\r\n");
    }
    let io = |error: std::io::Error| ExecError::new(error.to_string());
    write.write_all(&packed).await.map_err(io)?;
    write.flush().await.map_err(io)
}

/// Writes a reply as text, for a value inside another value.
fn frame_text(frame: &Frame) -> String {
    match frame {
        Frame::Simple(bytes) | Frame::Bulk(Some(bytes)) => {
            String::from_utf8_lossy(bytes).into_owned()
        }
        Frame::Error(message) => message.clone(),
        Frame::Int(number) => number.to_string(),
        Frame::Bulk(None) | Frame::Array(None) => "nil".to_owned(),
        Frame::Array(Some(items)) => {
            let items: Vec<String> = items.iter().map(frame_text).collect();
            format!("[{}]", items.join(", "))
        }
    }
}

fn write_frame(row: &mut RowWriter, frame: &Frame) {
    match frame {
        Frame::Bulk(None) | Frame::Array(None) => row.push(Value::Null),
        Frame::Int(number) => row.push(Value::Number(&number.to_string())),
        Frame::Simple(bytes) | Frame::Bulk(Some(bytes)) => {
            row.push(match std::str::from_utf8(bytes) {
                Ok(text) => Value::Text(text),
                Err(_) => Value::binary(bytes),
            })
        }
        other => row.push(Value::Text(&frame_text(other))),
    }
}

/// The local time of arrival, with milliseconds.
fn received_at() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

/// Returns the words of a reply list as text, such as `"SET" "k" "v"`.
fn command_text(items: &[Frame]) -> String {
    let words: Vec<Vec<u8>> = items
        .iter()
        .map(|item| match item {
            Frame::Simple(bytes) | Frame::Bulk(Some(bytes)) => bytes.clone(),
            other => frame_text(other).into_bytes(),
        })
        .collect();
    let words: Vec<&[u8]> = words.iter().map(Vec::as_slice).collect();
    RedisCommand(&words).to_string()
}

/// The columns of a `MONITOR` line. A line that does not match goes whole to `command`.
pub fn monitor_row(line: &str) -> Row {
    let mut row = RowWriter::default();
    let parts = line.split_once(" [").and_then(|(time, rest)| {
        let (source, command) = rest.split_once("] ")?;
        let (database, client) = source.split_once(' ')?;
        Some((time, database, client, command))
    });
    match parts {
        Some((time, database, client, command)) => {
            row.push(Value::Text(time));
            row.push(Value::Number(database));
            row.push(Value::Text(client));
            row.push(Value::Text(command));
        }
        None => {
            row.push(Value::Null);
            row.push(Value::Null);
            row.push(Value::Null);
            row.push(Value::Text(line));
        }
    }
    row.finish()
}

fn error_of(frame: Frame) -> Result<Frame, ExecError> {
    match frame {
        Frame::Error(message) => Err(ExecError::new(message)),
        other => Ok(other),
    }
}

/// Runs a continuous command until the user stops it or the connection fails. It returns
/// true when the grid received its columns.
pub(super) async fn run(
    target: &NetworkTarget,
    words: &[Vec<u8>],
    trace: Option<crate::command_log::TraceContext>,
    sink: &mut ResultSink,
) -> Result<bool, ExecError> {
    let stop = sink.stop_token();
    let mut shown = false;
    let result = tokio::select! {
        result = follow(target, words, trace, sink, &mut shown) => result,
        () = stop.cancelled() => Ok(()),
    };
    result.map(|()| shown)
}

async fn follow(
    target: &NetworkTarget,
    words: &[Vec<u8>],
    trace: Option<crate::command_log::TraceContext>,
    sink: &mut ResultSink,
    shown: &mut bool,
) -> Result<(), ExecError> {
    let stream = open_stream(target)
        .await
        .map_err(|error| ExecError::new(error.message))?;
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    // The stream sends no SELECT: Pub/Sub, MONITOR, and replication cross databases.
    if let Some(password) = &target.password {
        let mut auth: Vec<&[u8]> = vec![b"AUTH"];
        if !target.user.is_empty() {
            auth.push(target.user.as_bytes());
        }
        auth.push(password.expose().as_bytes());
        send(&mut write, &auth).await?;
        error_of(reader.frame().await?.0)?;
    }
    let refs: Vec<&[u8]> = words.iter().map(Vec::as_slice).collect();
    crate::command_log::observe(trace, RedisCommand(&refs), send(&mut write, &refs)).await?;
    let name = String::from_utf8_lossy(&words[0]).to_ascii_uppercase();
    match name.as_str() {
        "MONITOR" => monitor(&mut reader, sink, shown).await,
        "SYNC" | "PSYNC" => replicate(&mut reader, &mut write, name == "PSYNC", sink, shown).await,
        _ => subscribe(&mut reader, sink, shown).await,
    }
}

/// Sends the rows that wait when the next read can wait.
async fn flush_if_idle<R: AsyncRead + Unpin>(reader: &Reader<R>, sink: &mut ResultSink) {
    if !reader.has_buffered() {
        sink.flush().await;
    }
}

async fn subscribe<R: AsyncRead + Unpin>(
    reader: &mut Reader<R>,
    sink: &mut ResultSink,
    shown: &mut bool,
) -> Result<(), ExecError> {
    loop {
        flush_if_idle(reader, sink).await;
        let Frame::Array(Some(items)) = error_of(reader.frame().await?.0)? else {
            continue;
        };
        if !*shown {
            set_columns(
                &[
                    text::REDIS_COLUMN_RECEIVED_AT,
                    text::REDIS_COLUMN_KIND,
                    text::REDIS_COLUMN_PATTERN,
                    text::REDIS_COLUMN_CHANNEL,
                    text::REDIS_COLUMN_MESSAGE,
                ],
                sink,
            )
            .await;
            *shown = true;
        }
        let kind = items.first().map(frame_text).unwrap_or_default();
        let none = Frame::Bulk(None);
        let (pattern, channel, message) = match (kind.as_str(), items.get(1..).unwrap_or(&[])) {
            ("message" | "smessage", [channel, message]) => (&none, channel, message),
            ("pmessage", [pattern, channel, message]) => (pattern, channel, message),
            ("subscribe" | "psubscribe" | "ssubscribe", [channel, count]) => {
                let notice = text::redis_subscribed(&frame_text(channel), &frame_text(count));
                sink.notices(vec![notice], 0).await;
                continue;
            }
            _ => continue,
        };
        let mut row = RowWriter::default();
        row.push(Value::Text(&received_at()));
        row.push(Value::Text(&kind));
        write_frame(&mut row, pattern);
        write_frame(&mut row, channel);
        write_frame(&mut row, message);
        if sink.push(row.finish()).await != Flow::Continue {
            return Ok(());
        }
    }
}

async fn monitor<R: AsyncRead + Unpin>(
    reader: &mut Reader<R>,
    sink: &mut ResultSink,
    shown: &mut bool,
) -> Result<(), ExecError> {
    error_of(reader.frame().await?.0)?;
    set_columns(
        &[
            text::REDIS_COLUMN_TIME,
            text::REDIS_COLUMN_DATABASE,
            text::REDIS_COLUMN_CLIENT,
            text::REDIS_COLUMN_COMMAND,
        ],
        sink,
    )
    .await;
    *shown = true;
    loop {
        flush_if_idle(reader, sink).await;
        let line = frame_text(&error_of(reader.frame().await?.0)?);
        if sink.push(monitor_row(&line)).await != Flow::Continue {
            return Ok(());
        }
    }
}

fn sync_row(kind: &str, value: Value) -> Row {
    let mut row = RowWriter::default();
    row.push(Value::Text(&received_at()));
    row.push(Value::Text(kind));
    row.push(value);
    row.finish()
}

/// Shows a replication stream: the answer of `PSYNC`, the size of the RDB file, and then
/// each command. `PSYNC` acknowledges the offset each second, as a replica does.
async fn replicate<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut Reader<R>,
    write: &mut W,
    psync: bool,
    sink: &mut ResultSink,
    shown: &mut bool,
) -> Result<(), ExecError> {
    let columns = [
        text::REDIS_COLUMN_RECEIVED_AT,
        text::REDIS_COLUMN_KIND,
        text::REDIS_COLUMN_VALUE,
    ];
    let mut offset: i64 = 0;
    let mut full = !psync;
    if psync {
        let status = frame_text(&error_of(reader.frame().await?.0)?);
        set_columns(&columns, sink).await;
        *shown = true;
        let mut words = status.split(' ');
        full = words.next() == Some("FULLRESYNC");
        offset = words
            .next_back()
            .and_then(|word| word.parse().ok())
            .unwrap_or(0);
        if sink
            .push(sync_row(text::REDIS_SYNC_STATUS, Value::Text(&status)))
            .await
            != Flow::Continue
        {
            return Ok(());
        }
        sink.flush().await;
    }
    if full {
        let size = reader.skip_rdb().await?;
        if !*shown {
            set_columns(&columns, sink).await;
            *shown = true;
        }
        let size = size.to_string();
        if sink
            .push(sync_row(text::REDIS_SYNC_RDB, Value::Number(&size)))
            .await
            != Flow::Continue
        {
            return Ok(());
        }
    }
    let mut ack = tokio::time::interval(ACK_EVERY);
    loop {
        flush_if_idle(reader, sink).await;
        tokio::select! {
            frame = reader.frame() => {
                let (frame, size) = frame?;
                offset += i64::try_from(size).unwrap_or(i64::MAX);
                let text = match error_of(frame)? {
                    Frame::Array(Some(items)) => command_text(&items),
                    other => frame_text(&other),
                };
                if sink.push(sync_row(text::REDIS_SYNC_COMMAND, Value::Text(&text))).await != Flow::Continue {
                    return Ok(());
                }
            }
            _ = ack.tick(), if psync => {
                let offset = offset.to_string();
                send(write, &[b"REPLCONF", b"ACK", offset.as_bytes()]).await?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::{ExecEvent, StatementKey};
    use crate::grid::Budget;

    /// A reply list with no items must not stop the subscriber.
    #[test]
    fn an_empty_reply_list_does_not_stop_the_subscriber() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (mut write, read) = tokio::io::duplex(64);
            write
                .write_all(b"*0\r\n*3\r\n$7\r\nmessage\r\n$1\r\nc\r\n$2\r\nhi\r\n")
                .await
                .unwrap();
            drop(write);
            let (events, mut inbox) = tokio::sync::mpsc::channel(8);
            let budget = std::sync::Arc::new(Budget::new(1 << 20));
            let key = StatementKey {
                console: 1,
                execution: 1,
                index: 0,
            };
            let mut sink = ResultSink::new(key, events, budget, 10);
            let mut reader = Reader::new(read);
            let mut shown = false;
            let error = subscribe(&mut reader, &mut sink, &mut shown)
                .await
                .unwrap_err();
            assert!(error.message.contains("closed"), "{error:?}");
            let mut found = Vec::new();
            while let Ok(event) = inbox.try_recv() {
                match event {
                    ExecEvent::Columns { .. } => found.push("columns".to_owned()),
                    ExecEvent::Rows { rows, .. } => found.push(rows[0].get(4).unwrap().display()),
                    _ => {}
                }
            }
            assert_eq!(found, ["columns", "hi"], "the message still arrives");
        });
    }
}
