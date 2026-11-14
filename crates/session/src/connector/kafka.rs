//! The parts of the Kafka connector that need no driver library: the language, its
//! filter, and the names of the explorer.

pub mod command;
pub mod filter;

use serde_json::{Map, Value as Json};

use self::command::TOPIC_PAGE;
use super::words::command_word;
use crate::catalog::{ColumnNote, RelationRef};

/// The syntax guide of the Kafka language, for the AI completion and the MCP agents.
/// A test parses each example and checks that each function is in the guide.
pub const GUIDE: &str = include_str!("kafka/guide.md");
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

/// Writes each field path with its note as JSON, for the AI completion and the MCP
/// agents: the `path`, the `type` words, and the `values` of an enum.
pub fn field_json(paths: &[String], notes: &[ColumnNote]) -> Vec<Json> {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let mut field = Map::new();
            field.insert("path".into(), Json::from(path.as_str()));
            if let Some(note) = notes.get(index) {
                if !note.detail.is_empty() {
                    field.insert("type".into(), Json::from(note.detail.as_str()));
                }
                if !note.values.is_empty() {
                    field.insert("values".into(), Json::from(note.values.clone()));
                }
            }
            Json::Object(field)
        })
        .collect()
}
