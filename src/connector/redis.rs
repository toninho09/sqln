//! Redis and Valkey connector. Each statement is one command line, with the quote rules
//! of `redis-cli`. RESP3 gives the type of each reply, so a map reaches the grid as a map.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{
    AsyncConnectionConfig, Cmd, ErrorKind, ProtocolVersion, RedisConnectionInfo, RedisError,
    Value as Reply,
};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

use super::target::{Address, NetworkTarget, Tls};
use super::{
    Canceller, Capabilities, ConnectError, Connector, ExecError, Namespace, ResetOutcome, Session,
    SessionContext, StatementEnd, TransactionCommand, TransactionState, describe, tls,
};
use crate::catalog::{Catalog, Columns, Relation, RelationKind, RelationRef, Schema};
use crate::exec::{Flow, ResultSink};
use crate::grid::{Row, Value};
use crate::sql::Dialect;
use crate::ui::text;

/// A session call that gets no answer in this time counts as lost.
const SESSION_LIMIT: Duration = Duration::from_secs(5);
/// The explorer shows at most this count of keys in each database.
pub const MAX_KEYS: usize = 1_000;
/// The `COUNT` hint of each step of a `SCAN` family command.
const SCAN_STEP: &str = "1000";
/// The first word of the text that a key tab sends. It is not a server command.
const VALUE_PAGE: &str = "SQLN.VALUE";
/// These commands turn the connection into a stream that has no end.
const CONTINUOUS: &[&str] = &[
    "SUBSCRIBE",
    "PSUBSCRIBE",
    "SSUBSCRIBE",
    "MONITOR",
    "SYNC",
    "PSYNC",
];
/// These commands change the identity of a session that the pool gives to other consoles.
const IDENTITY: &[&str] = &["AUTH", "HELLO", "RESET"];

fn connect_failed(error: impl std::fmt::Display) -> ConnectError {
    ConnectError::new(error.to_string())
}

/// `NOAUTH` means that the server needs a password and the connection has none.
fn connect_error(error: &RedisError) -> ConnectError {
    let refused = error.kind() == ErrorKind::AuthenticationFailed
        || matches!(error.code(), Some("NOAUTH" | "WRONGPASS"));
    if refused {
        ConnectError::refused_password(describe(error))
    } else {
        ConnectError::new(describe(error))
    }
}

fn connection_info(target: &NetworkTarget) -> Result<RedisConnectionInfo, ConnectError> {
    let database = target
        .database
        .as_deref()
        .map_or(Ok(0), str::parse::<i64>)
        .map_err(|_| ConnectError::new(text::FORM_REDIS_DATABASE))?;
    let mut info = RedisConnectionInfo::default()
        .set_protocol(ProtocolVersion::RESP3)
        .set_db(database);
    if !target.user.is_empty() {
        info = info.set_username(&target.user);
    }
    if let Some(password) = &target.password {
        info = info.set_password(password.expose());
    }
    Ok(info)
}

/// Starts the driver on a stream. The runner holds the time limits, because the limits
/// of the driver would stop a command that waits, such as `BLPOP`.
async fn start<S>(
    info: &RedisConnectionInfo,
    stream: S,
) -> Result<MultiplexedConnection, ConnectError>
where
    S: Unpin + AsyncRead + AsyncWrite + Send + 'static,
{
    let config = AsyncConnectionConfig::new().set_response_timeout(None);
    let (conn, driver) = MultiplexedConnection::new_with_config(info, stream, config)
        .await
        .map_err(|error| connect_error(&error))?;
    tokio::spawn(driver);
    Ok(conn)
}

async fn open_tcp(
    info: &RedisConnectionInfo,
    host: &str,
    port: u16,
    tls_mode: &Tls,
) -> Result<MultiplexedConnection, ConnectError> {
    let stream = TcpStream::connect((host, port))
        .await
        .map_err(connect_failed)?;
    stream.set_nodelay(true).map_err(connect_failed)?;
    let Tls::Verify(files) = tls_mode else {
        return start(info, stream).await;
    };
    let config = tls::client_config(files)?;
    let name = ServerName::try_from(host.to_owned()).map_err(connect_failed)?;
    let stream = TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .map_err(connect_failed)?;
    start(info, stream).await
}

#[cfg(unix)]
async fn open_socket(
    info: &RedisConnectionInfo,
    path: &std::path::Path,
    tls_mode: &Tls,
) -> Result<MultiplexedConnection, ConnectError> {
    if matches!(tls_mode, Tls::Verify(_)) {
        return Err(ConnectError::new(text::REDIS_SOCKET_TLS));
    }
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(connect_failed)?;
    start(info, stream).await
}

#[cfg(not(unix))]
async fn open_socket(
    _info: &RedisConnectionInfo,
    _path: &std::path::Path,
    _tls_mode: &Tls,
) -> Result<MultiplexedConnection, ConnectError> {
    Err(ConnectError::new(text::REDIS_NO_SOCKET))
}

async fn open(target: &NetworkTarget) -> Result<MultiplexedConnection, ConnectError> {
    let info = connection_info(target)?;
    match &target.address {
        Address::Tcp { host, port } => open_tcp(&info, host, *port, &target.tls).await,
        Address::Socket(path) => open_socket(&info, path, &target.tls).await,
    }
}

pub struct RedisConnector {
    target: NetworkTarget,
}

impl RedisConnector {
    pub fn new(target: NetworkTarget) -> Self {
        Self { target }
    }
}

#[async_trait]
impl Connector for RedisConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Redis
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            namespace: Namespace::Database,
            transactions: false,
            table_filters: false,
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let mut conn = open(&self.target).await?;
        // An account without this right can run commands, but Stop then closes the session.
        let client = redis::cmd("CLIENT")
            .arg("ID")
            .query_async(&mut conn)
            .await
            .ok();
        let database = self.target.database.clone();
        Ok(Box::new(RedisSession {
            conn,
            canceller: Arc::new(RedisCanceller {
                target: self.target.clone(),
                client,
            }),
            database: database.unwrap_or_else(|| "0".to_owned()),
            read_only: false,
        }))
    }
}

struct RedisCanceller {
    target: NetworkTarget,
    /// The `CLIENT ID` of the session.
    client: Option<i64>,
}

impl RedisCanceller {
    /// The server can stop only a command that waits. Other commands answer 0.
    async fn unblock(&self) -> Result<(), String> {
        let client = self.client.ok_or(text::REDIS_NO_STOP)?;
        let mut conn = open(&self.target).await.map_err(|error| error.message)?;
        let stopped: i64 = redis::cmd("CLIENT")
            .arg("UNBLOCK")
            .arg(client)
            .arg("ERROR")
            .query_async(&mut conn)
            .await
            .map_err(|error| describe(&error))?;
        if stopped == 1 {
            Ok(())
        } else {
            Err(text::REDIS_NO_STOP.to_owned())
        }
    }
}

#[async_trait]
impl Canceller for RedisCanceller {
    async fn cancel(&self) -> Result<(), String> {
        match timeout(SESSION_LIMIT, self.unblock()).await {
            Ok(result) => result,
            Err(_) => Err("The cancel request got no answer.".to_owned()),
        }
    }
}

fn exec_error(error: &RedisError) -> ExecError {
    match (error.code(), error.detail()) {
        (Some(code), Some(detail)) => ExecError::new(format!("{code} {detail}")),
        _ => ExecError::new(describe(error)),
    }
}

fn bad_line(offset: usize) -> ExecError {
    ExecError {
        message: text::REDIS_BAD_QUOTES.to_owned(),
        offset: Some(offset),
    }
}

/// Reads the value of a `\x` escape. It returns `None` for any other text.
fn hex_escape(bytes: &[u8]) -> Option<u8> {
    let digits = std::str::from_utf8(bytes.get(..2)?).ok()?;
    u8::from_str_radix(digits, 16).ok()
}

/// Reads one word from `start`, as `redis-cli` does. It returns the word and the
/// position after it. A quote can start in the middle of a word, and a closing quote
/// ends the word.
fn read_word(bytes: &[u8], start: usize) -> Result<(Vec<u8>, usize), ExecError> {
    let mut word = Vec::new();
    let mut quote = None;
    let mut pos = start;
    loop {
        let Some(&byte) = bytes.get(pos) else {
            return if quote.is_some() {
                Err(bad_line(start))
            } else {
                Ok((word, pos))
            };
        };
        pos += 1;
        match (quote, byte) {
            (None, b' ' | b'\t' | b'\r' | b'\n') => return Ok((word, pos)),
            (None, b'"' | b'\'') => quote = Some(byte),
            (Some(open), _) if byte == open => {
                let follows_space = bytes.get(pos).is_none_or(u8::is_ascii_whitespace);
                return if follows_space {
                    Ok((word, pos))
                } else {
                    Err(bad_line(start))
                };
            }
            (Some(b'\''), b'\\') if bytes.get(pos) == Some(&b'\'') => {
                word.push(b'\'');
                pos += 1;
            }
            (Some(b'"'), b'\\') if bytes.get(pos) == Some(&b'x') => {
                match bytes.get(pos + 1..).and_then(hex_escape) {
                    Some(value) => {
                        word.push(value);
                        pos += 3;
                    }
                    None => {
                        word.push(b'x');
                        pos += 1;
                    }
                }
            }
            (Some(b'"'), b'\\') if pos < bytes.len() => {
                word.push(match bytes[pos] {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'b' => 8,
                    b'a' => 7,
                    other => other,
                });
                pos += 1;
            }
            _ => word.push(byte),
        }
    }
}

/// Splits a command line in words, as `redis-cli` does.
pub fn split_words(line: &str) -> Result<Vec<Vec<u8>>, ExecError> {
    let bytes = line.as_bytes();
    let mut words = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos].is_ascii_whitespace() {
            pos += 1;
            continue;
        }
        let (word, next) = read_word(bytes, pos)?;
        words.push(word);
        pos = next;
    }
    Ok(words)
}

/// Writes a word between double quotes, so that `split_words` reads the same text.
fn quoted_word(word: &str) -> String {
    let mut quoted = String::with_capacity(word.len() + 2);
    quoted.push('"');
    for c in word.chars() {
        match c {
            '"' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            c if c.is_ascii_control() => quoted.push_str(&format!("\\x{:02x}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// Returns the text of one page of a key tab. The session reads the type of the key at
/// each page, because a key can change between two pages.
pub fn value_page(relation: &RelationRef, offset: usize, count: usize) -> String {
    format!(
        "{VALUE_PAGE} {} {} {offset} {count}",
        relation.schema,
        quoted_word(&relation.relation)
    )
}

/// Writes a reply as text, for a value inside another value.
fn reply_text(reply: &Reply) -> String {
    let list = |items: &[Reply]| {
        let items: Vec<String> = items.iter().map(reply_text).collect();
        items.join(", ")
    };
    match reply {
        Reply::Nil => "nil".to_owned(),
        Reply::Okay => "OK".to_owned(),
        Reply::Int(number) => number.to_string(),
        Reply::Double(number) => number.to_string(),
        Reply::Boolean(value) => value.to_string(),
        Reply::SimpleString(text) => text.clone(),
        Reply::VerbatimString { text, .. } => text.clone(),
        Reply::BulkString(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Reply::Array(items) | Reply::Set(items) => format!("[{}]", list(items)),
        Reply::Map(pairs) => {
            let pairs: Vec<String> = pairs
                .iter()
                .map(|(key, value)| format!("{}: {}", reply_text(key), reply_text(value)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        Reply::Attribute { data, .. } => reply_text(data),
        other => format!("{other:?}"),
    }
}

/// Returns the value of one cell of the grid.
fn cell(reply: &Reply) -> Value {
    match reply {
        Reply::Nil => Value::Null,
        Reply::Int(number) => Value::Number(number.to_string().into()),
        Reply::Double(number) => Value::Number(number.to_string().into()),
        Reply::BulkString(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => Value::Text(text.into()),
            Err(_) => Value::Binary(bytes.as_slice().into()),
        },
        Reply::Attribute { data, .. } => cell(data),
        other => Value::Text(reply_text(other).into()),
    }
}

fn row(cells: &[&Reply]) -> Row {
    cells.iter().map(|reply| cell(reply)).collect()
}

/// Writes a reply as a grid, with its columns and rows. `None` is a reply without data:
/// `OK` reports only the end of a command.
pub fn reply_grid(reply: &Reply) -> Option<(Vec<&'static str>, Vec<Row>)> {
    match reply {
        Reply::Okay => None,
        Reply::Attribute { data, .. } => reply_grid(data),
        Reply::Array(items) | Reply::Set(items) => Some((
            vec![text::REDIS_COLUMN_VALUE],
            items.iter().map(|item| row(&[item])).collect(),
        )),
        Reply::Map(pairs) => Some((
            vec![text::REDIS_COLUMN_FIELD, text::REDIS_COLUMN_VALUE],
            pairs
                .iter()
                .map(|(key, value)| row(&[key, value]))
                .collect(),
        )),
        other => Some((vec![text::REDIS_COLUMN_VALUE], vec![row(&[other])])),
    }
}

/// Returns the items of a list or of a set. Any other reply has no items.
fn items(reply: &Reply) -> &[Reply] {
    match reply {
        Reply::Array(items) | Reply::Set(items) => items,
        _ => &[],
    }
}

/// Builds the rows of a hash or a set from the flat items of `SCAN`. A broken reply
/// can end with a group without its last values, so each row grows with nulls.
fn scan_rows(found: &[Reply], columns: usize) -> Vec<Row> {
    found
        .chunks(columns)
        .map(|cells| {
            cells
                .iter()
                .map(cell)
                .chain(std::iter::repeat(Value::Null))
                .take(columns)
                .collect()
        })
        .collect()
}

/// Reads the numbers of the databases with keys from the text of `INFO keyspace`.
pub fn keyspace_databases(info: &str) -> Vec<u32> {
    info.lines()
        .filter_map(|line| line.strip_prefix("db")?.split_once(':')?.0.parse().ok())
        .collect()
}

/// Sends rows to the sink. It returns false when the sink accepts no more rows.
async fn push_rows(rows: Vec<Row>, sink: &mut ResultSink) -> bool {
    for row in rows {
        if sink.push(row).await != Flow::Continue {
            return false;
        }
    }
    true
}

async fn set_columns(columns: &[&str], sink: &mut ResultSink) {
    sink.set_columns(columns.iter().map(|name| (*name).to_owned()).collect())
        .await;
}

/// Returns true when a reply had data for the grid.
async fn show(reply: &Reply, sink: &mut ResultSink) -> bool {
    let Some((columns, rows)) = reply_grid(reply) else {
        return false;
    };
    set_columns(&columns, sink).await;
    push_rows(rows, sink).await;
    sink.close_result().await;
    true
}

/// The part of a collection that one page of a key tab reads.
#[derive(Clone, Copy)]
struct Page {
    offset: usize,
    count: usize,
}

impl Page {
    fn last(self) -> usize {
        self.offset + self.count.saturating_sub(1)
    }
}

struct RedisSession {
    conn: MultiplexedConnection,
    canceller: Arc<RedisCanceller>,
    /// The database of the connection. A cleanup returns to it.
    database: String,
    read_only: bool,
}

impl RedisSession {
    async fn ask(&mut self, words: &[&[u8]]) -> Result<Reply, ExecError> {
        let mut command = Cmd::new();
        for word in words {
            command.arg(*word);
        }
        command
            .query_async(&mut self.conn)
            .await
            .map_err(|error| exec_error(&error))
    }

    async fn select(&mut self, database: &str) -> Result<(), ExecError> {
        self.ask(&[b"SELECT", database.as_bytes()]).await.map(drop)
    }

    /// Returns true when the server marks a command as a read. Any other answer returns
    /// false, so a server that does not answer refuses the command.
    async fn is_read(&mut self, name: &[u8]) -> bool {
        let Ok(reply) = self.ask(&[b"COMMAND", b"INFO", name]).await else {
            return false;
        };
        let flags = items(&reply)
            .first()
            .and_then(|info| items(info).get(2))
            .map_or(&[][..], items);
        flags
            .iter()
            .any(|flag| reply_text(flag).eq_ignore_ascii_case("readonly"))
    }

    async fn check(&mut self, name: &[u8]) -> Result<(), ExecError> {
        let upper = String::from_utf8_lossy(name).to_ascii_uppercase();
        if CONTINUOUS.contains(&upper.as_str()) {
            return Err(ExecError::new(text::REDIS_CONTINUOUS));
        }
        if IDENTITY.contains(&upper.as_str()) {
            return Err(ExecError::new(text::REDIS_IDENTITY));
        }
        if self.read_only && !self.is_read(name).await {
            return Err(ExecError::new(text::REDIS_READ_ONLY));
        }
        Ok(())
    }

    async fn read(&mut self, line: &str, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let words = split_words(line)?;
        let Some(name) = words.first() else {
            return Err(ExecError::new(text::REDIS_NO_COMMAND));
        };
        if name.as_slice() == VALUE_PAGE.as_bytes() {
            return self.read_value(&words, sink).await;
        }
        self.check(name).await?;
        let words: Vec<&[u8]> = words.iter().map(Vec::as_slice).collect();
        let reply = self.ask(&words).await?;
        Ok(show(&reply, sink).await)
    }

    /// Reads one page of a key, with the command of its type.
    async fn read_value(
        &mut self,
        words: &[Vec<u8>],
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        let number = |word: &[u8]| std::str::from_utf8(word).ok()?.parse::<usize>().ok();
        let [_, database, key, offset, count] = words else {
            return Err(ExecError::new(text::REDIS_BAD_PAGE));
        };
        let (Some(offset), Some(count)) = (number(offset), number(count)) else {
            return Err(ExecError::new(text::REDIS_BAD_PAGE));
        };
        let page = Page { offset, count };
        self.select(&String::from_utf8_lossy(database)).await?;
        let kind = reply_text(&self.ask(&[b"TYPE", key]).await?);
        match kind.as_str() {
            "string" => self.read_string(key, page, sink).await,
            "list" => self.read_list(key, page, sink).await,
            "hash" => {
                let columns = [text::REDIS_COLUMN_FIELD, text::REDIS_COLUMN_VALUE];
                self.read_scan(b"HSCAN", key, &columns, page, sink).await
            }
            "set" => {
                let columns = [text::REDIS_COLUMN_VALUE];
                self.read_scan(b"SSCAN", key, &columns, page, sink).await
            }
            "zset" => self.read_sorted(key, page, sink).await,
            "stream" => self.read_stream(key, page, sink).await,
            "none" => Err(ExecError::new(text::REDIS_NO_KEY)),
            other => Err(ExecError::new(text::redis_unknown_type(other))),
        }
    }

    async fn read_string(
        &mut self,
        key: &[u8],
        page: Page,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        set_columns(&[text::REDIS_COLUMN_VALUE], sink).await;
        if page.offset == 0 {
            let reply = self.ask(&[b"GET", key]).await?;
            sink.push(row(&[&reply])).await;
        }
        sink.close_result().await;
        Ok(true)
    }

    async fn read_list(
        &mut self,
        key: &[u8],
        page: Page,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        let (first, last) = (page.offset.to_string(), page.last().to_string());
        let reply = self
            .ask(&[b"LRANGE", key, first.as_bytes(), last.as_bytes()])
            .await?;
        Ok(show(&reply, sink).await)
    }

    async fn read_sorted(
        &mut self,
        key: &[u8],
        page: Page,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        let (first, last) = (page.offset.to_string(), page.last().to_string());
        let reply = self
            .ask(&[
                b"ZRANGE",
                key,
                first.as_bytes(),
                last.as_bytes(),
                b"WITHSCORES",
            ])
            .await?;
        let rows = items(&reply)
            .iter()
            .map(|pair| match items(pair) {
                [member, score] => row(&[member, score]),
                _ => row(&[pair, &Reply::Nil]),
            })
            .collect();
        set_columns(&[text::REDIS_COLUMN_MEMBER, text::REDIS_COLUMN_SCORE], sink).await;
        push_rows(rows, sink).await;
        sink.close_result().await;
        Ok(true)
    }

    /// `XRANGE` has no offset, so this page reads the entries before it again.
    async fn read_stream(
        &mut self,
        key: &[u8],
        page: Page,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        let total = (page.offset + page.count).to_string();
        let reply = self
            .ask(&[b"XRANGE", key, b"-", b"+", b"COUNT", total.as_bytes()])
            .await?;
        let rows = items(&reply)
            .iter()
            .skip(page.offset)
            .map(|entry| match items(entry) {
                [id, fields] => row(&[id, fields]),
                _ => row(&[entry, &Reply::Nil]),
            })
            .collect();
        set_columns(&[text::REDIS_COLUMN_ID, text::REDIS_COLUMN_FIELDS], sink).await;
        push_rows(rows, sink).await;
        sink.close_result().await;
        Ok(true)
    }

    /// Reads a hash or a set with its `SCAN` command. These commands have no offset, so
    /// a page repeats the steps before it, as `OFFSET` does in SQL.
    async fn read_scan(
        &mut self,
        verb: &[u8],
        key: &[u8],
        columns: &[&str],
        page: Page,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        set_columns(columns, sink).await;
        let mut cursor = "0".to_owned();
        let (mut skip, mut left) = (page.offset, page.count);
        loop {
            let reply = self
                .ask(&[verb, key, cursor.as_bytes(), b"COUNT", SCAN_STEP.as_bytes()])
                .await?;
            let [next, found] = items(&reply) else {
                return Err(ExecError::new(text::REDIS_BAD_REPLY));
            };
            let rows: Vec<Row> = scan_rows(items(found), columns.len())
                .into_iter()
                .skip(skip)
                .take(left)
                .collect();
            skip = skip.saturating_sub(items(found).len() / columns.len());
            left -= rows.len();
            let accepted = push_rows(rows, sink).await;
            cursor = reply_text(next);
            if left == 0 || cursor == "0" || !accepted {
                sink.close_result().await;
                return Ok(true);
            }
        }
    }

    /// Reads the keys of the current database, until the limit of the explorer. A key
    /// that is not UTF-8 has no name for the catalog, so it counts as hidden.
    async fn read_keys(&mut self) -> Result<(Vec<Relation>, bool), ExecError> {
        let mut names = Vec::new();
        let mut hidden = false;
        let mut cursor = "0".to_owned();
        loop {
            let reply = self
                .ask(&[b"SCAN", cursor.as_bytes(), b"COUNT", SCAN_STEP.as_bytes()])
                .await?;
            let [next, keys] = items(&reply) else {
                return Err(ExecError::new(text::REDIS_BAD_REPLY));
            };
            for key in items(keys) {
                match key {
                    Reply::BulkString(bytes) => match String::from_utf8(bytes.clone()) {
                        Ok(name) => names.push(name),
                        Err(_) => hidden = true,
                    },
                    _ => hidden = true,
                }
            }
            cursor = reply_text(next);
            names.sort_unstable();
            names.dedup();
            if cursor == "0" || names.len() >= MAX_KEYS {
                break;
            }
        }
        let partial = hidden || cursor != "0" || names.len() > MAX_KEYS;
        names.truncate(MAX_KEYS);
        let relations = names
            .into_iter()
            .map(|name| Relation {
                name,
                kind: RelationKind::Table,
                columns: Columns::NotLoaded,
                foreign_keys: Vec::new(),
            })
            .collect();
        Ok((relations, partial))
    }

    /// Removes what a statement left in the session. `DISCARD` fails outside `MULTI`,
    /// and that answer is normal.
    async fn clean(&mut self) -> bool {
        let _ = self.ask(&[b"DISCARD"]).await;
        let database = self.database.clone();
        self.ask(&[b"UNWATCH"]).await.is_ok() && self.select(&database).await.is_ok()
    }
}

#[async_trait]
impl Session for RedisSession {
    fn canceller(&self) -> Arc<dyn Canceller> {
        self.canceller.clone()
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        self.read_only = context.read_only;
        match &context.schema {
            Some(database) => self.select(database).await,
            None => Ok(()),
        }
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        let read = self.read(sql, &mut sink).await;
        let (rows, limit) = sink.finish().await;
        read.map(|has_columns| StatementEnd {
            rows,
            has_columns,
            changed: None,
            limit,
        })
    }

    async fn transaction_state(&mut self) -> TransactionState {
        TransactionState::None
    }

    async fn control(&mut self, _command: TransactionCommand) -> Result<(), ExecError> {
        Err(ExecError::new(text::REDIS_NO_TRANSACTIONS))
    }

    async fn reset(&mut self) -> ResetOutcome {
        match timeout(SESSION_LIMIT, self.clean()).await {
            Ok(true) => ResetOutcome::Clean,
            _ => ResetOutcome::Discard,
        }
    }

    async fn is_alive(&mut self) -> bool {
        matches!(
            timeout(SESSION_LIMIT, self.ask(&[b"PING"])).await,
            Ok(Ok(_))
        )
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let info = reply_text(&self.ask(&[b"INFO", b"keyspace"]).await?);
        let mut numbers: Vec<String> = keyspace_databases(&info)
            .iter()
            .map(u32::to_string)
            .collect();
        let current = self.database.clone();
        numbers.push(current.clone());
        numbers.sort_by_key(|number| number.parse::<u32>().unwrap_or(u32::MAX));
        numbers.dedup();
        let mut schemas = Vec::with_capacity(numbers.len());
        for name in numbers {
            self.select(&name).await?;
            let (relations, partial) = self.read_keys().await?;
            schemas.push(Schema {
                name,
                relations,
                partial,
            });
        }
        self.select(&current).await?;
        Ok(Catalog {
            schemas,
            current_schema: Some(current),
        })
    }

    /// A key has no columns.
    async fn read_columns(
        &mut self,
        _schema: &str,
        _relation: &str,
    ) -> Result<Vec<String>, ExecError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_group_of_a_scan_grows_with_nulls() {
        let item = |text: &str| Reply::BulkString(text.as_bytes().to_vec());
        let found = vec![item("a"), item("1"), item("b")];
        let rows = scan_rows(&found, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(
            rows[1].len(),
            2,
            "the row of a broken reply keeps its width"
        );
        assert!(matches!(rows[1][1], Value::Null));
    }
}
