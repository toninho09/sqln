//! Kafka connector, with the Schema Registry. Each statement is one command of SQLN, and
//! each command only reads. A read assigns the partitions by hand: SQLN never joins a
//! consumer group and never commits an offset, so the cluster never creates a group.

pub mod client;
pub mod command;
pub mod decode;
pub mod filter;
pub mod registry;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rdkafka::consumer::Consumer as _;
use rdkafka::error::{KafkaError, RDKafkaErrorCode};
use rdkafka::message::{Headers, Message as _};
use rdkafka::{Offset, TopicPartitionList};
use serde_json::Value as Json;
use tokio_util::sync::CancellationToken;

use self::client::{CALL_LIMIT, Consumer, Reader};
use self::command::{Command, Read, Start, TOPIC_PAGE};
use self::decode::{Decoded, decode};
use self::filter::{Filter, Message};
use self::registry::{RegistryClient, VaultRead};
use super::target::NetworkTarget;
use super::words::command_word;
use super::{
    Canceller, Capabilities, ConnectError, Connector, ExecError, MAX_NOTICES, Namespace,
    ResetOutcome, Session, SessionContext, StatementEnd, TransactionCommand, TransactionState,
};
use crate::catalog::{Catalog, Columns, Relation, RelationKind, RelationRef, Schema};
use crate::exec::{Flow, ResultSink};
use crate::grid::{Row, RowWriter, Value};
use crate::sql::Dialect;
use crate::ui::text;

pub use self::registry::account as registry_account;

/// The schema of the explorer that lists the topics.
pub const TOPICS: &str = "topics";
/// The schema of the explorer that lists the consumer groups.
pub const GROUPS: &str = "groups";
/// The messages of a topic tab.
pub const TOPIC_TAB_MESSAGES: usize = 500;
/// The columns of a message.
pub const MESSAGE_COLUMNS: [&str; 7] = [
    "partition",
    "offset",
    "timestamp",
    "key",
    "value",
    "headers",
    "schema",
];
/// The first window of offsets of a topic tab, for each partition. Each later window of
/// the same partition is two times larger, up to the largest window.
const FIRST_WINDOW: i64 = 500;
const LARGEST_WINDOW: i64 = 50_000;
/// A read without messages for this time sends its rows to the grid.
const IDLE: Duration = Duration::from_millis(200);

/// Returns the schema of the names after a command, for the completion.
pub fn argument_schema(command: &str) -> Option<&'static str> {
    match command.to_ascii_uppercase().as_str() {
        "CONSUME" | "TAIL" | "PARTITIONS" => Some(TOPICS),
        "GROUP" => Some(GROUPS),
        _ => None,
    }
}

/// Returns the text of a Kafka tab: the lag of a group, or the last messages of a topic.
pub fn page_text(relation: &RelationRef, filter: &str, count: usize) -> String {
    let name = command_word(&relation.relation);
    if relation.schema == GROUPS {
        return format!("GROUP {name}");
    }
    let mut page = format!("{TOPIC_PAGE} {name} {count}");
    let filter = filter.trim();
    if !filter.is_empty() {
        page.push_str(" WHERE ");
        page.push_str(filter);
    }
    page
}

/// Reads a registry password from the system vault.
fn system_vault() -> VaultRead {
    Arc::new(|account: &str| {
        crate::creds::SystemVault::new()
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
                ConnectError::new(message)
            }
        })?;
        Ok(Box::new(KafkaSession {
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

fn timestamp_text(millis: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|time| time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
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
    target: NetworkTarget,
    consumer: Arc<Consumer>,
    registry: Option<Arc<RegistryClient>>,
    registry_error: Option<String>,
}

/// The counts of a read, for the notice at its end.
#[derive(Default)]
struct Counts {
    scanned: u64,
    matched: u64,
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

    /// Returns the partitions of a topic with their low and high offsets.
    async fn offsets(
        &self,
        topic: &str,
        partition: Option<i32>,
    ) -> Result<Vec<(PartitionInfo, i64, i64)>, ExecError> {
        let info = self
            .metadata(Some(topic.to_owned()))
            .await?
            .into_iter()
            .find(|info| info.name == topic)
            .ok_or_else(|| ExecError::new(text::KAFKA_NO_TOPIC))?;
        if let Some(error) = info.error {
            return Err(ExecError::new(text::kafka_topic_error(&error)));
        }
        let mut partitions = info.partitions;
        partitions.sort_by_key(|partition| partition.id);
        if let Some(wanted) = partition {
            partitions.retain(|partition| partition.id == wanted);
            if partitions.is_empty() {
                return Err(ExecError::new(text::KAFKA_NO_PARTITION));
            }
        }
        let topic = topic.to_owned();
        client::blocking(&self.consumer, move |consumer| {
            partitions
                .into_iter()
                .map(|partition| {
                    let (low, high) =
                        consumer.fetch_watermarks(&topic, partition.id, CALL_LIMIT)?;
                    Ok((partition, low, high))
                })
                .collect()
        })
        .await
    }

    /// Returns the first offset of each partition for a start.
    async fn starts(
        &self,
        topic: &str,
        start: Start,
        partitions: &[(PartitionInfo, i64, i64)],
    ) -> Result<Vec<i64>, ExecError> {
        let clamp = |offset: i64, low: i64, high: i64| offset.clamp(low, high);
        match start {
            Start::Earliest => Ok(partitions.iter().map(|(_, low, _)| *low).collect()),
            Start::Latest => Ok(partitions.iter().map(|(_, _, high)| *high).collect()),
            Start::Offset(offset) => Ok(partitions
                .iter()
                .map(|(_, low, high)| clamp(offset, *low, *high))
                .collect()),
            Start::Time(millis) => {
                let mut list = TopicPartitionList::new();
                for (partition, _, _) in partitions {
                    list.add_partition_offset(topic, partition.id, Offset::Offset(millis))
                        .map_err(|error| ExecError::new(super::describe(&error)))?;
                }
                let found = client::blocking(&self.consumer, move |consumer| {
                    consumer.offsets_for_times(list, CALL_LIMIT).map(|list| {
                        list.elements()
                            .iter()
                            .map(|element| (element.partition(), element.offset()))
                            .collect::<Vec<_>>()
                    })
                })
                .await?;
                Ok(partitions
                    .iter()
                    .map(|(partition, low, high)| {
                        match found.iter().find(|(id, _)| *id == partition.id) {
                            Some((_, Offset::Offset(offset))) => clamp(*offset, *low, *high),
                            // No message at or after the time: the read starts at the end.
                            _ => *high,
                        }
                    })
                    .collect())
            }
        }
    }

    /// Decodes a message and checks the filter. It returns `None` for a message that does
    /// not match.
    async fn message_row(
        &self,
        raw: Raw,
        filter: Option<Arc<Filter>>,
        projection: Option<Arc<command::Projection>>,
        evaluation: &filter::Evaluation,
        notices: &mut Notices,
    ) -> Result<Option<Row>, ExecError> {
        let registry = self.registry.as_deref();
        let (key, key_notice) = decode(raw.key.as_deref(), registry).await;
        let (value, value_notice) = decode(raw.value.as_deref(), registry).await;
        key_notice
            .into_iter()
            .chain(value_notice)
            .for_each(|notice| notices.add(notice));
        let expressions = filter.is_some() || projection.is_some();
        let cancel_on_drop = evaluation.cancel_on_drop();
        let evaluation = evaluation.clone();
        let build = move || -> Result<Option<Row>, ExecError> {
            evaluation.check()?;
            let headers_json = headers_json(&raw.headers);
            let headers = headers_json.as_ref().map(Json::to_string);
            let schema = match (key.schema(), value.schema()) {
                (None, None) => None,
                (Some(key), None) => Some(format!("key {key}")),
                (None, Some(value)) => Some(format!("value {value}")),
                (Some(key), Some(value)) => Some(format!("key {key}, value {value}")),
            };
            if let Some(filter) = filter {
                let needs = filter.needs();
                let key_json = needs.key.then(|| key.json()).flatten();
                let value_json = needs.value.then(|| value.json()).flatten();
                let message = Message {
                    partition: raw.partition.into(),
                    offset: raw.offset,
                    timestamp: raw.timestamp,
                    key: key.text(),
                    key_json: key_json.as_deref(),
                    value: value.text(),
                    json: value_json.as_deref(),
                    headers: headers.as_deref(),
                    headers_json: headers_json.as_ref(),
                    schema: schema.as_deref(),
                };
                if !filter.matches(&message, &evaluation)? {
                    return Ok(None);
                }
            }
            let mut row = RowWriter::default();
            row.push(Value::Number(&raw.partition.to_string()));
            row.push(Value::Number(&raw.offset.to_string()));
            match raw.timestamp.and_then(timestamp_text) {
                Some(time) => row.push(Value::Text(&time)),
                None => row.push(Value::Null),
            }
            push_decoded(&mut row, &key);
            push_decoded(&mut row, &value);
            match &headers {
                Some(text) => row.push(Value::Text(text)),
                None => row.push(Value::Null),
            }
            match &schema {
                Some(text) => row.push(Value::Text(text)),
                None => row.push(Value::Null),
            }
            let row = row.finish();
            Ok(Some(match projection {
                Some(projection) => projection.apply(row, &evaluation)?,
                None => row,
            }))
        };
        // The runner remains free to process Stop and the statement deadline.
        // The visitor checks the stop token even when it produces no values.
        let result = if expressions {
            tokio::task::spawn_blocking(build)
                .await
                .map_err(|_| ExecError::new(text::KAFKA_CALL_FAILED))
                .and_then(|result| result)
        } else {
            build()
        };
        cancel_on_drop.disarm();
        result
    }

    fn registry_notice(&self, notices: &mut Notices) {
        if let Some(message) = &self.registry_error {
            notices.add(text::kafka_registry_unavailable(message));
        }
    }

    async fn topics(&self, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let mut topics = self.metadata(None).await?;
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        set_columns(&["topic", "partitions", "replicas", "internal"], sink).await;
        let rows = topics
            .iter()
            .map(|topic| {
                let replicas = topic.partitions.first().map_or(0, |p| p.replicas.len());
                number_row(&[
                    Some(topic.name.clone()),
                    Some(topic.partitions.len().to_string()),
                    Some(replicas.to_string()),
                    Some(is_internal(&topic.name).to_string()),
                ])
            })
            .collect();
        push_rows(rows, sink).await;
        Ok(true)
    }

    async fn partitions(&self, topic: &str, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let partitions = self.offsets(topic, None).await?;
        set_columns(
            &[
                "partition",
                "leader",
                "replicas",
                "isr",
                "low",
                "high",
                "messages",
            ],
            sink,
        )
        .await;
        let rows = partitions
            .iter()
            .map(|(partition, low, high)| {
                number_row(&[
                    Some(partition.id.to_string()),
                    Some(partition.leader.to_string()),
                    Some(list_text(&partition.replicas)),
                    Some(list_text(&partition.isr)),
                    Some(low.to_string()),
                    Some(high.to_string()),
                    Some((high - low).to_string()),
                ])
            })
            .collect();
        push_rows(rows, sink).await;
        Ok(true)
    }

    async fn group_list(&self, group: Option<String>) -> Result<Vec<GroupInfo>, ExecError> {
        client::blocking(&self.consumer, move |consumer| {
            consumer
                .fetch_group_list(group.as_deref(), CALL_LIMIT)
                .map(|list| groups_of(&list))
        })
        .await
    }

    async fn groups(&self, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let mut groups = self.group_list(None).await?;
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        set_columns(&["group", "state", "protocol", "members"], sink).await;
        let rows = groups
            .iter()
            .map(|group| {
                number_row(&[
                    Some(group.name.clone()),
                    Some(group.state.clone()),
                    Some(group.protocol.clone()).filter(|text| !text.is_empty()),
                    Some(group.members.len().to_string()),
                ])
            })
            .collect();
        push_rows(rows, sink).await;
        Ok(true)
    }

    /// Reads the committed offsets of a group and its lag. A consumer with the id of the
    /// group asks for the offsets only: it never joins the group.
    async fn group(&self, id: &str, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let members = self
            .group_list(Some(id.to_owned()))
            .await?
            .into_iter()
            .find(|group| group.name == id)
            .map(|group| group.members)
            .unwrap_or_default();
        let topics = self.metadata(None).await?;
        let mut list = TopicPartitionList::new();
        for topic in topics.iter().filter(|topic| !is_internal(&topic.name)) {
            for partition in &topic.partitions {
                list.add_partition(&topic.name, partition.id);
            }
        }
        let config =
            client::config(&self.target, id).map_err(|error| ExecError::new(error.message))?;
        let committed: Vec<(String, i32, i64, i64)> = tokio::task::spawn_blocking(move || {
            let consumer =
                client::consumer(&config).map_err(|error| ExecError::new(error.message))?;
            let found = consumer
                .committed_offsets(list, CALL_LIMIT)
                .map_err(|error| ExecError::new(client::call_error(&consumer, &error).1))?;
            let mut rows = Vec::new();
            for element in found.elements() {
                let Offset::Offset(offset) = element.offset() else {
                    continue;
                };
                let (_, high) = consumer
                    .fetch_watermarks(element.topic(), element.partition(), CALL_LIMIT)
                    .map_err(|error| ExecError::new(client::call_error(&consumer, &error).1))?;
                rows.push((
                    element.topic().to_owned(),
                    element.partition(),
                    offset,
                    high,
                ));
            }
            Ok::<_, ExecError>(rows)
        })
        .await
        .map_err(|_| ExecError::new(text::KAFKA_CALL_FAILED))??;
        let mut committed = committed;
        committed.sort();
        set_columns(
            &["topic", "partition", "committed", "end", "lag", "member"],
            sink,
        )
        .await;
        let rows = committed
            .iter()
            .map(|(topic, partition, offset, high)| {
                let member = members
                    .iter()
                    .find(|member| {
                        member
                            .assignment
                            .iter()
                            .any(|(name, id)| name == topic && id == partition)
                    })
                    .map(|member| member.client.clone());
                number_row(&[
                    Some(topic.clone()),
                    Some(partition.to_string()),
                    Some(offset.to_string()),
                    Some(high.to_string()),
                    Some((high - offset).max(0).to_string()),
                    member,
                ])
            })
            .collect();
        push_rows(rows, sink).await;
        if committed.is_empty() {
            sink.notices(vec![text::KAFKA_GROUP_EMPTY.to_owned()], 0)
                .await;
        }
        Ok(true)
    }

    /// Runs `CONSUME` or `TAIL`. `CONSUME` ends at the last offset of each partition at its
    /// start; `TAIL` waits for new messages until the user stops it.
    async fn read(&self, read: Read, tail: bool, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let partitions = self.offsets(&read.topic, read.partition).await?;
        let starts = self.starts(&read.topic, read.start, &partitions).await?;
        let windows: Vec<Window> = partitions
            .iter()
            .zip(&starts)
            .map(|((partition, _, high), start)| Window {
                partition: partition.id,
                start: *start,
                end: (!tail).then_some(*high),
            })
            .collect();
        if let Some(projection) = &read.projection {
            sink.set_columns(projection.names()).await;
        } else {
            set_columns(&MESSAGE_COLUMNS, sink).await;
        }
        let mut notices = Notices::default();
        self.registry_notice(&mut notices);
        let stop = sink.stop_token();
        let evaluation = filter::Evaluation::new(stop.clone(), sink.memory_limit());
        let filter = read.filter.map(Arc::new);
        let projection = read.projection.map(Arc::new);
        let mut scan = Scan::start(&self.target, &read.topic, &windows, !tail)?;
        let mut counts = Counts::default();
        let mut stopped = false;
        loop {
            match scan.next(&stop).await? {
                Next::Message(raw) => {
                    counts.scanned += 1;
                    let row = self
                        .message_row(
                            raw,
                            filter.clone(),
                            projection.clone(),
                            &evaluation,
                            &mut notices,
                        )
                        .await;
                    if evaluation.stopped() {
                        stopped = true;
                        break;
                    }
                    if let Some(row) = row? {
                        counts.matched += 1;
                        if sink.push(row).await != Flow::Continue {
                            break;
                        }
                        if read
                            .limit
                            .is_some_and(|limit| counts.matched >= limit as u64)
                        {
                            break;
                        }
                    }
                    if read.scan.is_some_and(|scan| counts.scanned >= scan) {
                        break;
                    }
                }
                Next::Idle => sink.flush().await,
                Next::Done => break,
                Next::Stopped => {
                    stopped = true;
                    break;
                }
            }
        }
        drop(scan);
        if filter.is_some() || read.scan.is_some() || stopped {
            notices.add(text::kafka_scanned(counts.scanned, counts.matched, stopped));
        }
        notices.send(sink).await;
        Ok(true)
    }

    /// Reads the last messages of a topic that match the filter, newest first. Each round
    /// reads an older window of each partition that can still hold a newer match.
    async fn topic_page(
        &self,
        topic: &str,
        count: usize,
        filter: Option<Filter>,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        set_columns(&MESSAGE_COLUMNS, sink).await;
        if count == 0 {
            return Ok(true);
        }
        let partitions = self.offsets(topic, None).await?;
        let mut notices = Notices::default();
        self.registry_notice(&mut notices);
        let stop = sink.stop_token();
        let evaluation = filter::Evaluation::new(stop.clone(), sink.memory_limit());
        let filter = filter.map(Arc::new);
        let mut ends: HashMap<i32, i64> = partitions
            .iter()
            .map(|(p, _, high)| (p.id, *high))
            .collect();
        let lows: HashMap<i32, i64> = partitions.iter().map(|(p, low, _)| (p.id, *low)).collect();
        let mut sizes: HashMap<i32, i64> = lows.keys().map(|id| (*id, FIRST_WINDOW)).collect();
        let mut oldest: HashMap<i32, i64> = lows.keys().map(|id| (*id, i64::MAX)).collect();
        let mut found: Vec<(i64, i32, i64, Row)> = Vec::new();
        let mut counts = Counts::default();
        let mut stopped = false;
        loop {
            found.sort_by_key(|item| std::cmp::Reverse((item.0, item.1, item.2)));
            found.truncate(count);
            let threshold = (found.len() >= count).then(|| found[found.len() - 1].0);
            let windows: Vec<Window> = partitions
                .iter()
                .map(|(partition, _, _)| partition.id)
                .filter(|id| ends[id] > lows[id])
                .filter(|id| threshold.is_none_or(|time| oldest[id] >= time))
                .map(|id| Window {
                    partition: id,
                    start: (ends[&id] - sizes[&id]).max(lows[&id]),
                    end: Some(ends[&id]),
                })
                .collect();
            if windows.is_empty() || stopped {
                break;
            }
            let mut scan = Scan::start(&self.target, topic, &windows, true)?;
            loop {
                match scan.next(&stop).await? {
                    Next::Message(raw) => {
                        counts.scanned += 1;
                        let time = raw.timestamp.unwrap_or(-1);
                        let (partition, offset) = (raw.partition, raw.offset);
                        oldest
                            .entry(raw.partition)
                            .and_modify(|oldest| *oldest = (*oldest).min(time));
                        let row = self
                            .message_row(raw, filter.clone(), None, &evaluation, &mut notices)
                            .await;
                        if evaluation.stopped() {
                            stopped = true;
                            break;
                        }
                        if let Some(row) = row? {
                            counts.matched += 1;
                            found.push((time, partition, offset, row));
                        }
                    }
                    Next::Idle => {}
                    Next::Done => break,
                    Next::Stopped => {
                        stopped = true;
                        break;
                    }
                }
            }
            drop(scan);
            for window in &windows {
                ends.insert(window.partition, window.start);
                sizes
                    .entry(window.partition)
                    .and_modify(|size| *size = (*size * 2).min(LARGEST_WINDOW));
            }
        }
        found.sort_by_key(|item| std::cmp::Reverse((item.0, item.1, item.2)));
        found.truncate(count);
        push_rows(found.into_iter().map(|(.., row)| row).collect(), sink).await;
        if filter.is_some() || stopped {
            notices.add(text::kafka_scanned(counts.scanned, counts.matched, stopped));
        }
        notices.send(sink).await;
        Ok(true)
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
    fn canceller(&self) -> Arc<dyn Canceller> {
        Arc::new(KafkaCanceller)
    }

    /// Each command only reads, and Kafka has no schemas to choose.
    async fn apply_context(&mut self, _context: &SessionContext) -> Result<(), ExecError> {
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        let read = self.run_command(sql, &mut sink).await;
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
        Err(ExecError::new(text::KAFKA_NO_TRANSACTIONS))
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
        };
        let mut topics: Vec<String> = self
            .metadata(None)
            .await?
            .into_iter()
            .map(|topic| topic.name)
            .filter(|name| !is_internal(name))
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

    /// A topic and a group have no columns.
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
