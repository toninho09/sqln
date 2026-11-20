//! Kafka connector, with the Schema Registry. Each statement is one command of SQLN, and
//! each command only reads. A read assigns the partitions by hand: SQLN never joins a
//! consumer group and never commits an offset, so the cluster never creates a group.

pub mod client;
pub mod decode;
pub mod fields;
pub mod registry;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rdkafka::consumer::Consumer as _;
use rdkafka::error::{KafkaError, RDKafkaErrorCode};
use rdkafka::message::{Headers, Message as _};
use rdkafka::{Offset, TopicPartitionList};
use serde_json::Value as Json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use self::client::{CALL_LIMIT, Consumer, Reader};
use self::decode::{Decoded, decode};
use self::registry::{RegistryClient, VaultRead};
use super::target::{NetworkTarget, topic_visible};
use super::{
    Canceller, Capabilities, ConnectError, Connector, ErrorKind, ExecError, MAX_NOTICES, Namespace,
    ResetOutcome, Session, SessionContext, StatementEnd, TransactionCommand, TransactionState,
};
use crate::catalog::{Catalog, ColumnNote, Columns, Relation, RelationKind, Schema};
use crate::connector::kafka::command::{Command, Read, Start, time_text};
use crate::connector::kafka::filter::{Filter, Message, Meta};
use crate::connector::kafka::{GROUPS, MESSAGE_COLUMNS, TOPICS, command, filter};
use crate::grid::{Row, RowWriter, Value};
use crate::progress::{ReadEnd, ReadProgress, percent};
use crate::sink::{Flow, ResultSink};
use crate::sql::Dialect;
use crate::text;
use sqln_session::trace::{self, TraceContext};

pub use sqln_session::connector::kafka::registry_account;

mod explorer;
mod read;

/// The first window of offsets of a topic tab, for each partition. Each later window of
/// the same partition is two times larger, up to the largest window.
const FIRST_WINDOW: i64 = 500;
const LARGEST_WINDOW: i64 = 50_000;
/// A read without messages for this time sends its rows to the grid.
const IDLE: Duration = Duration::from_millis(200);
/// The time between two reads of the high offsets during `TAIL`, for its lag.
const LAG_EVERY: Duration = Duration::from_secs(2);

/// Reads a registry password from the system vault.
fn system_vault() -> VaultRead {
    Arc::new(|account: &str| {
        sqln_vault::SystemVault::new()
            .read_api_key(account)
            .map_err(|error| text::kafka_registry_vault_error(&error.to_string()))
    })
}

pub struct KafkaConnector {
    target: NetworkTarget,
    registry: Option<Arc<RegistryClient>>,
    /// The reason that the registry client did not start.
    registry_error: Option<String>,
}

impl KafkaConnector {
    pub fn new(target: NetworkTarget) -> Self {
        Self::with_vault(target, system_vault())
    }

    /// Uses another vault for the password of the registry, for the tests.
    pub fn with_vault(target: NetworkTarget, vault: VaultRead) -> Self {
        let (registry, registry_error) = match &target.kafka.registry {
            None => (None, None),
            Some(registry) => match RegistryClient::new(registry, vault) {
                Ok(client) => (Some(Arc::new(client)), None),
                Err(message) => (None, Some(message)),
            },
        };
        Self {
            target,
            registry,
            registry_error,
        }
    }
}

#[async_trait]
impl Connector for KafkaConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Kafka
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            namespace: Namespace::None,
            transactions: false,
            table_filters: false,
            data_edit: false,
            parameters: false,
            single_session: false,
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let config = client::config(&self.target, self.target.kafka.read_group())?;
        let consumer = Arc::new(client::consumer(&config)?);
        // One metadata call checks the address, TLS, and the password.
        let check = consumer.clone();
        tokio::task::spawn_blocking(move || {
            check
                .fetch_metadata(None, CALL_LIMIT)
                .map(drop)
                .map_err(|error| client::call_error(&check, &error))
        })
        .await
        .map_err(|_| ConnectError::new(text::KAFKA_CALL_FAILED))?
        .map_err(|(refused, message)| {
            if refused {
                ConnectError::refused_password(message)
            } else {
                ConnectError::new(message).kind(ErrorKind::Connection)
            }
        })?;
        Ok(Box::new(KafkaSession {
            trace: None,
            target: self.target.clone(),
            consumer,
            registry: self.registry.clone(),
            registry_error: self.registry_error.clone(),
        }))
    }
}

/// A read watches the stop token of its sink, so the request has nothing to do.
struct KafkaCanceller;

#[async_trait]
impl Canceller for KafkaCanceller {
    async fn cancel(&self) -> Result<(), String> {
        Ok(())
    }
}

/// One partition of a topic, with its offsets.
#[derive(Clone)]
struct PartitionInfo {
    id: i32,
    leader: i32,
    replicas: Vec<i32>,
    isr: Vec<i32>,
}

struct TopicInfo {
    name: String,
    partitions: Vec<PartitionInfo>,
    error: Option<String>,
}

fn topics_of(metadata: &rdkafka::metadata::Metadata) -> Vec<TopicInfo> {
    metadata
        .topics()
        .iter()
        .map(|topic| TopicInfo {
            name: topic.name().to_owned(),
            partitions: topic
                .partitions()
                .iter()
                .map(|partition| PartitionInfo {
                    id: partition.id(),
                    leader: partition.leader(),
                    replicas: partition.replicas().to_vec(),
                    isr: partition.isr().to_vec(),
                })
                .collect(),
            error: topic
                .error()
                .map(|error| RDKafkaErrorCode::from(error).to_string()),
        })
        .collect()
}

/// Internal topics, such as `__consumer_offsets`, stay out of the explorer.
fn is_internal(name: &str) -> bool {
    name.starts_with("__")
}

struct Member {
    client: String,
    assignment: Vec<(String, i32)>,
}

struct GroupInfo {
    name: String,
    state: String,
    protocol: String,
    members: Vec<Member>,
}

/// Reads the partitions of a member of a consumer group. The format is the one of the
/// consumer protocol: a version, then each topic with its partitions.
fn assignment(bytes: &[u8]) -> Vec<(String, i32)> {
    fn take<'a>(bytes: &mut &'a [u8], count: usize) -> Option<&'a [u8]> {
        let (head, tail) = bytes.split_at_checked(count)?;
        *bytes = tail;
        Some(head)
    }
    fn number(bytes: &mut &[u8]) -> Option<i32> {
        take(bytes, 4).map(|four| i32::from_be_bytes([four[0], four[1], four[2], four[3]]))
    }
    let mut bytes = bytes;
    let mut found = Vec::new();
    let read = (|| {
        take(&mut bytes, 2)?;
        let topics = number(&mut bytes)?;
        for _ in 0..topics.max(0) {
            let length = take(&mut bytes, 2).map(|two| i16::from_be_bytes([two[0], two[1]]))?;
            let name = take(&mut bytes, usize::try_from(length).ok()?)?;
            let name = String::from_utf8_lossy(name).into_owned();
            for _ in 0..number(&mut bytes)?.max(0) {
                found.push((name.clone(), number(&mut bytes)?));
            }
        }
        Some(())
    })();
    let _ = read;
    found
}

fn groups_of(list: &rdkafka::groups::GroupList) -> Vec<GroupInfo> {
    list.groups()
        .iter()
        .map(|group| GroupInfo {
            name: group.name().to_owned(),
            state: group.state().to_owned(),
            protocol: group.protocol().to_owned(),
            members: group
                .members()
                .iter()
                .map(|member| Member {
                    client: format!("{} {}", member.client_id(), member.client_host()),
                    assignment: if group.protocol_type() == "consumer" {
                        member.assignment().map(assignment).unwrap_or_default()
                    } else {
                        Vec::new()
                    },
                })
                .collect(),
        })
        .collect()
}

/// A message, with its bytes copied out of librdkafka.
struct Raw {
    partition: i32,
    offset: i64,
    timestamp: Option<i64>,
    key: Option<Vec<u8>>,
    value: Option<Vec<u8>>,
    headers: Vec<(String, Option<Vec<u8>>)>,
}

impl Raw {
    /// Returns the bytes of the key, the value, and the headers.
    fn size(&self) -> u64 {
        let length = |bytes: &Option<Vec<u8>>| bytes.as_ref().map_or(0, Vec::len);
        let headers: usize = self
            .headers
            .iter()
            .map(|(name, value)| name.len() + length(value))
            .sum();
        u64::try_from(length(&self.key) + length(&self.value) + headers).unwrap_or(u64::MAX)
    }
}

/// Where a read starts and ends in one partition. A read without an end waits for new
/// messages until the user stops it.
struct Window {
    partition: i32,
    start: i64,
    end: Option<i64>,
}

enum Next {
    Message(Raw),
    /// No message arrived for a short time.
    Idle,
    /// Each partition reached its end.
    Done,
    Stopped,
}

/// One read of a topic. It owns its consumer, so a later read starts clean.
struct Scan {
    reader: Option<Reader>,
    ends: HashMap<i32, i64>,
    open: HashSet<i32>,
    bounded: bool,
}

impl Scan {
    fn start(
        target: &NetworkTarget,
        topic: &str,
        windows: &[Window],
        bounded: bool,
    ) -> Result<Self, ExecError> {
        let config = client::config(target, target.kafka.read_group())
            .map_err(|error| ExecError::new(error.message))?;
        let mut list = TopicPartitionList::new();
        let mut ends = HashMap::new();
        let mut open = HashSet::new();
        for window in windows {
            if window.end.is_some_and(|end| window.start >= end) {
                continue;
            }
            list.add_partition_offset(topic, window.partition, Offset::Offset(window.start))
                .map_err(|error| ExecError::new(super::describe(&error)))?;
            if let Some(end) = window.end {
                ends.insert(window.partition, end);
            }
            open.insert(window.partition);
        }
        let reader = if open.is_empty() {
            None
        } else {
            let reader = client::reader(&config)?;
            reader
                .assign(&list)
                .map_err(|error| ExecError::new(super::describe(&error)))?;
            Some(reader)
        };
        Ok(Self {
            reader,
            ends,
            open,
            bounded,
        })
    }

    async fn next(&mut self, stop: &CancellationToken) -> Result<Next, ExecError> {
        loop {
            let Some(reader) = &self.reader else {
                return Ok(Next::Done);
            };
            if self.bounded && self.open.is_empty() {
                return Ok(Next::Done);
            }
            let received = tokio::select! {
                biased;
                () = stop.cancelled() => return Ok(Next::Stopped),
                received = tokio::time::timeout(IDLE, reader.recv()) => received,
            };
            let message = match received {
                Err(_) => return Ok(Next::Idle),
                Ok(Err(KafkaError::PartitionEOF(partition))) => {
                    if self.bounded {
                        self.open.remove(&partition);
                    }
                    continue;
                }
                Ok(Err(error)) => return Err(ExecError::new(super::describe(&error))),
                Ok(Ok(message)) => message,
            };
            let (partition, offset) = (message.partition(), message.offset());
            if !self.open.contains(&partition) {
                continue;
            }
            if let Some(end) = self.ends.get(&partition).copied() {
                if offset >= end {
                    self.open.remove(&partition);
                    continue;
                }
                if offset + 1 >= end {
                    self.open.remove(&partition);
                }
            }
            let headers = message
                .headers()
                .map(|headers| {
                    headers
                        .iter()
                        .map(|header| (header.key.to_owned(), header.value.map(<[u8]>::to_vec)))
                        .collect()
                })
                .unwrap_or_default();
            return Ok(Next::Message(Raw {
                partition,
                offset,
                timestamp: message.timestamp().to_millis(),
                key: message.key().map(<[u8]>::to_vec),
                value: message.payload().map(<[u8]>::to_vec),
                headers,
            }));
        }
    }
}

/// The close of a consumer can wait for the network, so it happens on a thread for
/// blocking work.
impl Drop for Scan {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn_blocking(move || drop(reader));
        }
    }
}

/// The position of a read in each partition. It gives the part of a finite read that is
/// done, and the lag of a `TAIL`: the messages that it still has to read.
struct Position {
    /// The next offset of each partition.
    next: HashMap<i32, i64>,
    /// The end of a finite read, or the last known high offset of a `TAIL`.
    ends: HashMap<i32, i64>,
    /// The offsets from the next offset to the end, for all partitions.
    left: i64,
    /// The offsets from the start to the end, for all partitions.
    total: i64,
}

impl Position {
    fn new(windows: &[Window], ends: impl IntoIterator<Item = (i32, i64)>) -> Self {
        let next: HashMap<i32, i64> = windows
            .iter()
            .map(|window| (window.partition, window.start))
            .collect();
        let mut position = Self {
            next,
            ends: HashMap::new(),
            left: 0,
            total: 0,
        };
        position.set_ends(ends);
        position.total = position.left;
        position
    }

    fn left_in(next: i64, end: i64) -> i64 {
        (end - next).max(0)
    }

    /// Moves the position of a partition after a message.
    fn advance(&mut self, partition: i32, offset: i64) {
        let Some(next) = self.next.get_mut(&partition) else {
            return;
        };
        let after = offset.saturating_add(1);
        if after <= *next {
            return;
        }
        if let Some(end) = self.ends.get(&partition).copied() {
            self.left -= Self::left_in(*next, end) - Self::left_in(after, end);
        }
        *next = after;
    }

    /// Replaces the ends with new high offsets.
    fn set_ends(&mut self, ends: impl IntoIterator<Item = (i32, i64)>) {
        self.ends = ends.into_iter().collect();
        self.left = self
            .next
            .iter()
            .filter_map(|(partition, next)| {
                let end = self.ends.get(partition)?;
                Some(Self::left_in(*next, *end))
            })
            .sum();
    }

    fn left(&self) -> u64 {
        u64::try_from(self.left).unwrap_or(0)
    }

    fn done(&self) -> u64 {
        u64::try_from(self.total - self.left).unwrap_or(0)
    }

    fn total(&self) -> u64 {
        u64::try_from(self.total).unwrap_or(0)
    }
}

/// The partitions, with their low and high offsets.
type Watermarks = Vec<(i32, i64, i64)>;

/// Returns the low and high offsets of each partition.
async fn watermarks(
    consumer: &Arc<Consumer>,
    topic: &str,
    partitions: Vec<i32>,
) -> Result<Watermarks, ExecError> {
    let topic = topic.to_owned();
    client::blocking(consumer, move |consumer| {
        partitions
            .into_iter()
            .map(|id| {
                let (low, high) = consumer.fetch_watermarks(&topic, id, CALL_LIMIT)?;
                Ok((id, low, high))
            })
            .collect()
    })
    .await
}

/// Reads the high offsets of a `TAIL` again from time to time, for its lag. The read of
/// the messages never waits for it.
struct LagCheck {
    consumer: Arc<Consumer>,
    topic: String,
    partitions: Vec<i32>,
    task: Option<JoinHandle<Result<Watermarks, ExecError>>>,
    started: Instant,
}

impl LagCheck {
    fn new(consumer: Arc<Consumer>, topic: &str, partitions: Vec<i32>) -> Self {
        Self {
            consumer,
            topic: topic.to_owned(),
            partitions,
            task: None,
            started: Instant::now(),
        }
    }

    /// Returns the answer of the last check when it is complete, and starts a new check
    /// at the end of `LAG_EVERY`. `Some(None)` is a failed check.
    async fn poll(&mut self) -> Option<Option<Vec<(i32, i64)>>> {
        if self.task.as_ref().is_some_and(JoinHandle::is_finished) {
            let task = self.task.take()?;
            let highs = task.await.ok().and_then(Result::ok).map(|marks| {
                marks
                    .into_iter()
                    .map(|(partition, _, high)| (partition, high))
                    .collect()
            });
            return Some(highs);
        }
        if self.task.is_none() && self.started.elapsed() >= LAG_EVERY {
            let consumer = self.consumer.clone();
            let topic = self.topic.clone();
            let partitions = self.partitions.clone();
            self.task = Some(tokio::spawn(async move {
                watermarks(&consumer, &topic, partitions).await
            }));
            self.started = Instant::now();
        }
        None
    }
}

/// Reads the headers as a JSON object. A repeated name holds a list. A value that is not
/// UTF-8 becomes `0x` and its hex digits.
fn headers_json(headers: &[(String, Option<Vec<u8>>)]) -> Option<Json> {
    if headers.is_empty() {
        return None;
    }
    let mut map = serde_json::Map::new();
    for (name, value) in headers {
        let value = match value {
            None => Json::Null,
            Some(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) => Json::String(text.to_owned()),
                Err(_) => {
                    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                    Json::String(format!("0x{hex}"))
                }
            },
        };
        let repeated = headers.iter().filter(|(other, _)| other == name).count() > 1;
        if !repeated {
            map.insert(name.clone(), value);
        } else if let Json::Array(items) = map
            .entry(name.clone())
            .or_insert_with(|| Json::Array(Vec::new()))
        {
            items.push(value);
        }
    }
    Some(Json::Object(map))
}

fn push_decoded(row: &mut RowWriter, decoded: &Decoded) {
    match decoded {
        Decoded::Null => row.push(Value::Null),
        Decoded::Text(text) => row.push(Value::Text(text)),
        Decoded::Bytes(bytes) => row.push(Value::binary(bytes)),
        Decoded::Json { text, .. } => row.push(Value::Text(text)),
    }
}

/// The notices of a read, without repeats and with a limit.
#[derive(Default)]
struct Notices {
    texts: Vec<String>,
    more: usize,
}

impl Notices {
    fn add(&mut self, text: String) {
        if self.texts.contains(&text) {
            return;
        }
        if self.texts.len() < MAX_NOTICES {
            self.texts.push(text);
        } else {
            self.more += 1;
        }
    }

    async fn send(self, sink: &mut ResultSink) {
        if !self.texts.is_empty() || self.more > 0 {
            sink.notices(self.texts, self.more).await;
        }
    }
}

async fn set_columns(columns: &[&str], sink: &mut ResultSink) {
    sink.set_columns(columns.iter().map(|name| (*name).to_owned()).collect())
        .await;
}

fn list_text(numbers: &[i32]) -> String {
    numbers
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn number_row(cells: &[Option<String>]) -> Row {
    let mut row = RowWriter::default();
    for cell in cells {
        match cell {
            Some(text) if text.parse::<f64>().is_ok() => row.push(Value::Number(text)),
            Some(text) => row.push(Value::Text(text)),
            None => row.push(Value::Null),
        }
    }
    row.finish()
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

struct KafkaSession {
    trace: Option<TraceContext>,
    target: NetworkTarget,
    consumer: Arc<Consumer>,
    registry: Option<Arc<RegistryClient>>,
    registry_error: Option<String>,
}

impl KafkaSession {
    async fn metadata(&self, topic: Option<String>) -> Result<Vec<TopicInfo>, ExecError> {
        client::blocking(&self.consumer, move |consumer| {
            consumer
                .fetch_metadata(topic.as_deref(), CALL_LIMIT)
                .map(|metadata| topics_of(&metadata))
        })
        .await
    }

    async fn run_command(&self, line: &str, sink: &mut ResultSink) -> Result<bool, ExecError> {
        match command::parse(line)? {
            Command::Topics => self.topics(sink).await,
            Command::Partitions(topic) => self.partitions(&topic, sink).await,
            Command::Groups => self.groups(sink).await,
            Command::Group(id) => self.group(&id, sink).await,
            Command::Consume(read) => self.read(read, false, sink).await,
            Command::Tail(read) => self.read(read, true, sink).await,
            Command::TopicPage {
                topic,
                count,
                filter,
            } => self.topic_page(&topic, count, filter, sink).await,
        }
    }
}

#[async_trait]
impl Session for KafkaSession {
    fn set_trace_context(&mut self, trace: Option<TraceContext>) {
        self.trace = trace;
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        Arc::new(KafkaCanceller)
    }

    /// Each command only reads, and Kafka has no schemas to choose.
    async fn apply_context(&mut self, _context: &SessionContext) -> Result<(), ExecError> {
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        let read = trace::observe(self.trace.clone(), sql, self.run_command(sql, &mut sink)).await;
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
        Err(ExecError::with_kind(
            ErrorKind::Unsupported,
            text::KAFKA_NO_TRANSACTIONS,
        ))
    }

    /// A read owns its consumer, so the session keeps no state between statements.
    async fn reset(&mut self) -> ResetOutcome {
        ResetOutcome::Clean
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let relation = |name: String| Relation {
            name,
            kind: RelationKind::Table,
            columns: Columns::NotLoaded,
            foreign_keys: Vec::new(),
            referencing: Vec::new(),
            notes: Vec::new(),
        };
        let mut topics: Vec<String> = self
            .metadata(None)
            .await?
            .into_iter()
            .map(|topic| topic.name)
            .filter(|name| !is_internal(name))
            // The topic filter of the connection changes only the catalog: TOPICS and
            // the reads still reach the other topics.
            .filter(|name| topic_visible(&self.target.kafka.topics, name))
            .collect();
        topics.sort();
        let mut groups: Vec<String> = self
            .group_list(None)
            .await?
            .into_iter()
            .map(|group| group.name)
            .collect();
        groups.sort();
        Ok(Catalog {
            schemas: vec![
                Schema {
                    name: TOPICS.to_owned(),
                    relations: topics.into_iter().map(relation).collect(),
                    ..Schema::default()
                },
                Schema {
                    name: GROUPS.to_owned(),
                    relations: groups.into_iter().map(relation).collect(),
                    ..Schema::default()
                },
            ],
            current_schema: Some(TOPICS.to_owned()),
        })
    }

    async fn read_schema_names(&mut self) -> Result<Vec<String>, ExecError> {
        Ok(vec![TOPICS.to_owned(), GROUPS.to_owned()])
    }

    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, ExecError> {
        Ok(self.read_noted_columns(schema, relation).await?.0)
    }

    /// The columns of a topic are the field paths of the latest schemas of its value and
    /// its key, such as `value.customer.id`. Each note holds the type words and the enum
    /// values of its path. A group and a topic without a schema have no columns.
    async fn read_noted_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<(Vec<String>, Vec<ColumnNote>), ExecError> {
        if schema != TOPICS {
            return Ok((Vec::new(), Vec::new()));
        }
        if let Some(message) = &self.registry_error {
            return Err(ExecError::new(text::kafka_registry_unavailable(message)));
        }
        let Some(registry) = &self.registry else {
            return Ok((Vec::new(), Vec::new()));
        };
        let (mut paths, mut notes) = (Vec::new(), Vec::new());
        for root in ["value", "key"] {
            let subject = format!("{relation}-{root}");
            let latest = registry.latest(&subject).await.map_err(ExecError::new)?;
            if let Some(parsed) = latest {
                let (found, found_notes) = fields::paths(root, &parsed);
                paths.extend(found);
                notes.extend(found_notes);
            }
        }
        Ok((paths, notes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::RelationRef;
    use crate::connector::kafka::page_text;

    #[tokio::test]
    async fn a_failed_user_command_emits_its_command_and_error_without_a_broker() {
        use crate::connector::target::parse_uri;
        use crate::sink::{SinkEvent, StatementKey};
        use sqln_session::trace::{LogEvent, TraceEvent};

        let target = parse_uri("kafka://127.0.0.1:1").unwrap();
        let config = client::config(&target, target.kafka.read_group()).unwrap();
        let mut session = KafkaSession {
            trace: None,
            target,
            consumer: Arc::new(client::consumer(&config).unwrap()),
            registry: None,
            registry_error: None,
        };
        let key = StatementKey {
            console: 1,
            execution: 2,
            index: 0,
        };
        let (events, mut traces) = tokio::sync::mpsc::channel::<TraceEvent>(8);
        let mut trace = TraceContext::new(1, 1, "Kafka".into(), false, events);
        trace.key = Some(key);
        trace.user = true;
        trace.dialect = Dialect::Kafka;
        session.set_trace_context(Some(trace));
        let (events, _rows) = tokio::sync::mpsc::channel::<SinkEvent>(8);
        let sink = ResultSink::new(key, events, Arc::new(crate::grid::Budget::new(4096)), 10);
        let command = "INVALID command";
        let error = session.run(command, sink).await.unwrap_err();
        let TraceEvent {
            event:
                LogEvent::Started {
                    id,
                    key: recorded_key,
                    user,
                    dialect,
                    text,
                    header_len,
                    ..
                },
            ..
        } = traces.try_recv().unwrap()
        else {
            panic!("the command starts a trace");
        };
        assert_eq!(recorded_key, Some(key));
        assert!(user);
        assert_eq!(dialect, Dialect::Kafka);
        assert_eq!(&text[header_len..], command);
        let TraceEvent {
            event:
                LogEvent::Finished {
                    id: finished,
                    level,
                    text,
                    ..
                },
            ..
        } = traces.try_recv().unwrap()
        else {
            panic!("the failed command ends its trace");
        };
        assert_eq!(finished, id);
        assert_eq!(level, sqln_core::message::MessageLevel::Error);
        assert!(text.contains(&error.to_string()), "{text}");
        assert!(traces.try_recv().is_err());
    }

    #[test]
    fn an_assignment_lists_each_partition_of_each_topic() {
        let mut bytes = vec![0, 0, 0, 0, 0, 2];
        for (name, partitions) in [("a", vec![0_i32, 2]), ("bc", vec![1])] {
            bytes.extend_from_slice(&(name.len() as i16).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(&(partitions.len() as i32).to_be_bytes());
            for partition in partitions {
                bytes.extend_from_slice(&partition.to_be_bytes());
            }
        }
        assert_eq!(
            assignment(&bytes),
            vec![("a".into(), 0), ("a".into(), 2), ("bc".into(), 1)]
        );
        assert!(
            assignment(&[0, 0, 0]).is_empty(),
            "a short answer gives nothing"
        );
    }

    #[test]
    fn headers_become_json_and_keep_repeats_and_bytes() {
        let headers = vec![
            ("trace".to_owned(), Some(b"t1".to_vec())),
            ("tag".to_owned(), Some(b"a".to_vec())),
            ("tag".to_owned(), Some(vec![0xff])),
            ("empty".to_owned(), None),
        ];
        assert_eq!(
            headers_json(&headers).unwrap(),
            serde_json::json!({"trace": "t1", "tag": ["a", "0xff"], "empty": null})
        );
        assert_eq!(headers_json(&[]), None);
    }

    #[test]
    fn a_tab_text_reads_a_group_or_the_last_messages_of_a_topic() {
        let topic = RelationRef {
            schema: TOPICS.into(),
            relation: "orders v1".into(),
        };
        assert_eq!(page_text(&topic, "", 500), "SQLN.TOPIC \"orders v1\" 500");
        assert_eq!(
            page_text(&topic, " key = 'a' ", 10),
            "SQLN.TOPIC \"orders v1\" 10 WHERE key = 'a'"
        );
        let group = RelationRef {
            schema: GROUPS.into(),
            relation: "billing".into(),
        };
        assert_eq!(page_text(&group, "ignored", 500), "GROUP billing");
        assert!(matches!(
            command::parse(&page_text(&topic, "value.a > 1", 7)).unwrap(),
            Command::TopicPage {
                count: 7,
                filter: Some(_),
                ..
            }
        ));
    }
}
