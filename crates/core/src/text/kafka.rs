//! Kafka.

use super::{SUMMARY_GAP, byte_size, count, plural, wide};
use crate::progress::ProgressSnapshot;
use crate::progress::ReadEnd;

pub const FORM_REGISTRY: &str = "Schema Registry";
pub const FORM_KAFKA_SOCKET: &str = "A Kafka connection needs a host name, not a socket.";
pub const KAFKA_EVAL_MEMORY: &str = "The Kafka expression exceeds the temporary memory limit. Reduce the projection or increase the memory limit.";
pub const KAFKA_EVAL_STOPPED: &str =
    "The Kafka expression stops because the execution is canceled.";
pub const KAFKA_BAD_GROUP: &str =
    "The consumer group cannot hold a space or a control character. Remove them.";
pub const AGENT_KAFKA_DESCRIBE: &str = "A Kafka describe reads the fields of one topic. Send an object with the topic name and the kind topic, or send no object to read the catalog.";
pub const AGENT_KAFKA_NO_FIELDS: &str = "This topic has no fields: the connection has no Schema Registry, or the registry has no subject <topic>-value or <topic>-key. Read a few messages, such as SELECT * FROM <topic> LIMIT 5, to see the paths of key and value. Do not guess paths.";
pub const AGENT_KAFKA_SCAN_EMPTY: &str = "The read reached its SCAN before any message matched. The rest of the topic was not read: raise SCAN, or start nearer the messages with START AT or a time condition.";
/// The note of an agent read that SQLN bounded.
pub fn agent_default_scan(scan: u64) -> String {
    format!(
        "SQLN added SCAN {scan} to the agent read, because it had no SCAN. LIMIT counts only the matches, and START AT does not end the read."
    )
}
pub const AGENT_KAFKA_GUIDE_SENT: &str = "The Kafka guide was in an earlier get_context reply of this session. Call get_context with include_syntax to read it again.";
pub const KAFKA_BAD_TOPICS: &str = "Separate the topic patterns with commas, such as orders.*, !*.dlq. A pattern cannot hold a space or a control character, and ! needs a pattern.";
pub const FORM_REGISTRY_URL: &str =
    "The registry URL must start with http:// or https://. A registry user needs a URL.";

pub const KAFKA_UNKNOWN_COMMAND: &str =
    "Unknown Kafka command. Use SELECT, TOPICS, PARTITIONS, CONSUME, TAIL, GROUPS, or GROUP.";
pub const KAFKA_SELECT_START: &str =
    "START AT needs EARLIEST, LATEST, OFFSET n, or TIMESTAMP 'UTC time'.";
pub const KAFKA_SELECT_CLAUSES: &str = "SELECT accepts PARTITION, START AT, WHERE, LIMIT, and SCAN. End a continuous read with EMIT CHANGES, without LIMIT or SCAN.";
pub const KAFKA_EXTRA_WORDS: &str = "The command has more words than it accepts. Remove them.";
pub const KAFKA_CONSUME_CLAUSES: &str =
    "CONSUME accepts PARTITION, FROM, WHERE, LIMIT, and SCAN after the topic.";
pub const KAFKA_TAIL_CLAUSES: &str = "TAIL accepts PARTITION, FROM, and WHERE after the topic.";
pub const KAFKA_REPEATED_CLAUSE: &str = "Each clause can occur one time. Remove the repeat.";
pub const KAFKA_BAD_START: &str = "FROM needs earliest, latest, an offset of 0 or more, or @ and a time, such as @2026-11-10T12:00:00Z.";
pub const KAFKA_BAD_TEXT: &str = "The text has a bad escape or is not UTF-8.";
pub const KAFKA_BAD_OPERATOR: &str = "Use != for not equal.";
pub const KAFKA_NEEDS_FIELD: &str = "Use a message column, such as key or value, a field, such as value.customer.id, or a function, such as LOWER(value.name).";
pub const KAFKA_NEEDS_OPERATOR: &str = "Put =, !=, <, <=, >, >=, IS NULL, IN, BETWEEN, LIKE, ILIKE, CONTAINS, or MATCHES after the value.";
pub const KAFKA_NEEDS_LIST: &str = "Put a field that holds a list here, such as value.tags, or a list of values, such as ('a', 'b').";
pub const KAFKA_NEEDS_NULL: &str = "Write IS NULL, IS NOT NULL, IS MISSING, or IS NOT MISSING.";
pub const KAFKA_NEEDS_AND: &str = "Write BETWEEN low AND high, such as BETWEEN 1 AND 5.";
pub const KAFKA_NEEDS_NUMBER: &str =
    "This value is a number. Compare it with a number, such as SIZE(value.items) > 2.";
pub const KAFKA_TEXT_OPERATOR: &str =
    "CONTAINS, MATCHES, LIKE, and ILIKE read a text. Use =, <, or > for a number.";
pub const KAFKA_BAD_TIME: &str =
    "Compare a time with a UTC time, such as '2026-11-10T12:00:00Z' or '2026-11-10'.";
pub const KAFKA_NEEDS_TIME: &str = "This function needs a time, such as timestamp or NOW(). Convert a number with TO_TIMESTAMP(seconds) or TO_TIMESTAMP_MS(milliseconds).";
pub const KAFKA_NEEDS_LITERAL: &str =
    "Put a text between quotes here, such as ','. SQLN reads it before the read starts.";
pub const KAFKA_BAD_INTERVAL: &str = "Write an interval with numbers and units, such as INTERVAL '1 hour 30 minutes'. The units are ms, s, min, h, d, and week.";
pub const KAFKA_INTERVAL_MONTH: &str = "An interval cannot use months or years, because their length changes. Use days, such as INTERVAL '30 days'.";
pub const KAFKA_BAD_FLAGS: &str =
    "REGEXP_REPLACE accepts the flags g (all matches) and i (ignore case), such as 'gi'.";
pub const KAFKA_CASE: &str = "Write CASE WHEN condition THEN value, more WHEN parts if necessary, an optional ELSE value, and END.";
pub const KAFKA_LIKE_END: &str =
    "The LIKE pattern ends with a backslash. Write \\\\ for one backslash in the text.";
pub const KAFKA_CAST_TYPE: &str =
    "Write CAST(value AS NUMBER), CAST(value AS TEXT), or CAST(value AS BOOLEAN).";
pub const KAFKA_ITEM_NAME: &str = "Give the item a new simple name that is not a message column, such as ANY item IN value.items.";
pub const KAFKA_QUANTIFIER: &str =
    "Write ANY item IN value.list SATISFIES condition END, or EVERY in place of ANY.";
pub const KAFKA_SIZE_FIELD: &str =
    "SIZE needs a value without [*] between parentheses, such as SIZE(value.items).";
pub const KAFKA_NEEDS_TEXT: &str = "The condition needs a text between quotes.";
pub const KAFKA_NEEDS_VALUE: &str =
    "Put a value here: a field, a text between quotes, a number, true, false, or null.";
pub const KAFKA_NO_CLOSE: &str = "A parenthesis does not close. Add ).";
pub const KAFKA_BAD_REGEX: &str =
    "The regular expression is not valid or is too large. Check its syntax.";
pub const KAFKA_WHAT_TOPIC: &str = "a topic";
pub const KAFKA_WHAT_GROUP: &str = "a group";
pub const KAFKA_WHAT_PARTITION: &str = "a partition number";
pub const KAFKA_WHAT_COUNT: &str = "a number of 1 or more";
pub const KAFKA_NO_TOPIC: &str = "The topic does not exist. Use TOPICS to see the topics.";
pub const KAFKA_NO_PARTITION: &str =
    "The topic has no partition with this number. Use PARTITIONS to see them.";
pub const KAFKA_NO_TRANSACTIONS: &str =
    "Kafka has no transactions in SQLN. Each command only reads.";
pub const KAFKA_CALL_FAILED: &str = "The Kafka call stopped before its end. Try again.";
pub const KAFKA_GROUP_EMPTY: &str =
    "The group has no committed offsets. A group that does not exist also gives this result.";
pub const KAFKA_PATH_TEXT: &str =
    "librdkafka needs a TLS file path in UTF-8. Move or rename the file.";
pub const KAFKA_NO_ROOTS: &str =
    "The system has no TLS roots for Kafka. Set a CA file in the Security section.";
pub const KAFKA_DECODE_FAILED: &str = "the payload does not match its schema";
pub const KAFKA_REGISTRY_CLIENT: &str = "SQLN cannot create the Schema Registry client.";
pub const KAFKA_REGISTRY_UNREACHABLE: &str =
    "SQLN cannot reach the Schema Registry. Check the URL, the network, and TLS.";
pub const KAFKA_REGISTRY_REFUSED: &str =
    "the Schema Registry refused the credentials. Check the registry user and password";
pub const KAFKA_REGISTRY_NO_SCHEMA: &str = "the Schema Registry has no schema with this id";
pub const KAFKA_REGISTRY_BAD_ANSWER: &str =
    "the Schema Registry sent an answer that SQLN cannot read";
pub const KAFKA_REGISTRY_BAD_SCHEMA: &str = "SQLN cannot read the schema of the registry";
pub const KAFKA_REGISTRY_DEEP: &str = "the schema has too many levels of references";
pub const KAFKA_REGISTRY_VAULT: &str = "SQLN cannot read the registry password from the vault";

/// Names what a Kafka command needs at a position.
pub fn kafka_needs(what: &str) -> String {
    format!("The command needs {what} here.")
}

/// Tells how many values a function takes. `max` is `usize::MAX` for no limit.
pub fn kafka_function_needs(name: &str, min: usize, max: usize) -> String {
    let count = |value: usize| match value {
        1 => "one".to_owned(),
        2 => "two".to_owned(),
        3 => "three".to_owned(),
        4 => "four".to_owned(),
        other => other.to_string(),
    };
    let values = if min == 1 && max == 1 {
        "value"
    } else {
        "values"
    };
    let amount = match (min, max) {
        (0, 0) => return format!("{name} takes no values. Write {name}()."),
        (min, usize::MAX) => format!("{} or more", count(min)),
        (min, max) if min == max => count(min),
        (min, max) => format!("{} to {}", count(min), count(max)),
    };
    let commas = if max > 1 {
        ", with commas between them"
    } else {
        ""
    };
    format!("{name} needs {amount} {values} between parentheses{commas}.")
}

pub fn kafka_registry_not_saved(error: &str) -> String {
    format!(
        "SQLN did not save the Schema Registry password in the vault: {error} Edit the connection and save again."
    )
}

pub fn kafka_registry_status(status: u16) -> String {
    format!("the Schema Registry answered HTTP {status}")
}

pub fn kafka_registry_vault_error(error: &str) -> String {
    format!("SQLN cannot read the registry password from the vault: {error}")
}

pub fn kafka_registry_unavailable(reason: &str) -> String {
    format!("The Schema Registry is not available: {reason}. Values stay as text or bytes.")
}

/// A payload with a schema id that the registry could not decode keeps its bytes.
pub fn kafka_decode_notice(id: u32, reason: &str) -> String {
    format!("Schema {id}: {reason}. The value stays as bytes.")
}

pub fn kafka_topic_error(error: &str) -> String {
    format!("The cluster refused the topic: {error}.")
}

/// The counts of a read with a filter, a scan limit, or a stop.
pub fn kafka_scanned(scanned: u64, matched: u64, stopped: bool) -> String {
    let end = if stopped {
        " The read stopped before the end."
    } else {
        ""
    };
    format!("Scanned {scanned} messages; {matched} matched.{end}")
}

/// The counts of a Kafka read that runs, by importance: a narrow screen cuts the end.
pub fn kafka_progress(progress: &ProgressSnapshot, rate: Option<u64>) -> String {
    let mut parts = vec![format!(
        "{} scanned",
        plural(wide(progress.scanned), "message")
    )];
    if progress.filtered {
        parts.push(format!("{} matched", count(wide(progress.matched))));
    }
    if let Some(lag) = progress.lag {
        parts.push(format!("lag {}", count(wide(lag))));
    }
    if let Some(rate) = rate {
        parts.push(format!("{} msg/s", count(wide(rate))));
    }
    parts.push(byte_size(progress.bytes));
    parts.join(SUMMARY_GAP)
}

/// The summary of a finite Kafka read that runs.
pub fn kafka_reading(progress: &ProgressSnapshot, rate: Option<u64>) -> String {
    let state = match progress.percent {
        Some(percent) => format!("Reading {percent}%"),
        None => "Reading".to_owned(),
    };
    format!("{state}{SUMMARY_GAP}{}", kafka_progress(progress, rate))
}

/// The counts at the end of a Kafka read, and the reason for the end.
pub fn kafka_read_end(progress: &ProgressSnapshot) -> String {
    let reason = match progress.end {
        ReadEnd::None => "",
        ReadEnd::End => " The read reached the end of each partition.",
        ReadEnd::Limit => " The read stopped at LIMIT.",
        ReadEnd::Scan => " The read stopped at the SCAN limit.",
        ReadEnd::Stopped => " The read stopped before the end.",
    };
    format!(
        "{} scanned, {}.{reason}",
        plural(wide(progress.scanned), "message"),
        byte_size(progress.bytes)
    )
}
pub const FIELD_SASL: &str = "SASL";
pub const FIELD_CONSUMER_GROUP: &str = "Consumer group";
pub const FIELD_TOPICS: &str = "Topics";
pub const FIELD_MAX_SCAN: &str = "Max scan";
pub const KAFKA_BAD_MAX_SCAN: &str =
    "Enter a number of messages greater than 0, or leave it empty to use Max scan of the settings.";
pub const FIELD_REGISTRY_URL: &str = "Registry URL";
pub const FIELD_REGISTRY_USER: &str = "Registry user";
pub const FIELD_REGISTRY_PASSWORD: &str = "Registry password";
