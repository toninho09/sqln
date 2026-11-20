//! The assistant contract. These types contain no interface or driver state.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: u32 = 3;
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
pub const PAGE_BYTES: usize = 256 * 1024;
pub const TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
impl Error {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
pub type Reply = Result<Value, Error>;

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct Position {
    /// The line, from 0.
    pub line: usize,
    /// The column in Unicode characters, from 0.
    pub column: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    /// The first character to replace.
    pub start: Position,
    /// The end of the replaced text, excluded. Equal to `start` to insert.
    pub end: Position,
    /// The new text.
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Range {
    /// The first character of the range.
    pub start: Position,
    /// The end of the range, excluded.
    pub end: Position,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditMode {
    #[default]
    Propose,
    Direct,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// The parameter name, without its marker, such as `id` for `:id`.
    pub name: String,
    /// The type of the value.
    pub kind: BindingKind,
    /// The value as text: `true`/`false` for a boolean, decimal text for a number.
    #[serde(default)]
    pub value: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    Null,
    Boolean,
    Number,
    Text,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Side {
    /// The connection_id from get_context.
    pub connection_id: String,
    /// The schema to compare.
    pub schema: String,
    /// One table of the schema. Required for a data comparison.
    pub table: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DdlKind {
    Table,
    View,
    Function,
    Procedure,
    Sequence,
    Trigger,
    Enum,
    Domain,
    /// A Kafka topic: `describe` reads its fields. `schema` is optional.
    Topic,
}
impl DdlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::View => "view",
            Self::Function => "function",
            Self::Procedure => "procedure",
            Self::Sequence => "sequence",
            Self::Trigger => "trigger",
            Self::Enum => "enum",
            Self::Domain => "domain",
            Self::Topic => "topic",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplainMode {
    /// Show the plan without execution.
    Plan,
    /// Execute the statement and show the measured plan.
    Analyze,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompareKind {
    Structure,
    Data,
}

/// A database object for `describe`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Object {
    /// The object name, without its schema.
    pub name: String,
    /// The kind of the object. `topic` for a Kafka topic.
    pub kind: DdlKind,
    /// The arguments of a PostgreSQL routine, or the table of a trigger.
    pub identity: Option<String>,
}

/// Each variant becomes one MCP tool. Fields are deliberately explicit.
/// Tools that start a task wait for its end in the bridge, so the agent receives the result.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Request access in SQLN and wait for the user's decision. Call it again to read the state.
    /// The level is `propose` (edits need review), `direct` (direct edits), or `full` (database writes).
    RequestAccess {},
    /// Read the access level, the active tab, the connections, the open tabs, and pending proposals.
    /// With a Kafka connection, the first reply of the session holds the Kafka guide in `syntax.Kafka`.
    GetContext {
        /// The first tab to list. Use `next_offset` of the previous reply.
        #[serde(default)]
        offset: usize,
        /// The most tabs to list, from 1 to 1,000.
        #[serde(default = "page_rows")]
        limit: usize,
        /// The first connection to list. A reply lists up to 200 connections: use
        /// `connections_next_offset` of the previous reply.
        #[serde(default)]
        connection_offset: usize,
        /// Send the Kafka guide again, for example after the client shortened its context.
        #[serde(default)]
        include_syntax: bool,
    },
    /// Read a console text page and its revision. Offsets count UTF-8 bytes.
    /// Set `messages` to also read its Messages from record `message_offset`.
    ReadConsole {
        /// The console_id from get_context.
        console_id: String,
        /// The first byte of the page. Use `next_offset` of the previous page.
        #[serde(default)]
        offset: usize,
        /// The revision from the first page. Required for later pages.
        revision: Option<u64>,
        /// Also read the Messages of the console: notices, errors, and agent notes.
        #[serde(default)]
        messages: bool,
        /// The first Messages record. Use `next_offset` of the previous messages.
        #[serde(default)]
        message_offset: usize,
    },
    /// Read loaded result rows without executing SQL. Reuse the returned generation for later pages.
    /// With `row` and `column`, read one large cell in blocks from `offset` bytes. Binary blocks use base64.
    ReadResult {
        /// The tab_id from get_context or from a task reply.
        tab_id: String,
        /// The result of the tab, from 0. A script has one result for each statement with rows.
        #[serde(default)]
        result: usize,
        /// The first row, or the first byte of a cell with `row` and `column`.
        #[serde(default)]
        offset: usize,
        /// The most rows, from 1 to 1,000.
        #[serde(default = "page_rows")]
        limit: usize,
        /// The generation of the first page. Required for later pages and cells.
        generation: Option<String>,
        /// The row of one cell, from 0. Send it with `column`.
        row: Option<usize>,
        /// The column of one cell, from 0. Send it with `row`.
        column: Option<usize>,
    },
    /// Execute SQL on a connection in this session's agent console and return its results.
    /// Write `sql` in the dialect of the connection: a Kafka connection follows syntax.Kafka
    /// of get_context, and refuses TAIL and EMIT CHANGES because they do not end.
    /// Without full access, only known reads run. Use `edit_console` for SQL that the user must run.
    Query {
        /// The connection_id from get_context.
        connection_id: String,
        /// The schema or database that the statements use by default.
        schema: Option<String>,
        /// One statement or a script, in the dialect of the connection.
        sql: String,
        /// Values of the named parameters of `sql`, such as `:id`.
        #[serde(default)]
        parameters: Vec<Binding>,
        /// Explain one statement instead of running it as is.
        explain: Option<ExplainMode>,
        /// Rows of each result in the reply. The default is 100.
        rows: Option<usize>,
    },
    /// Execute the current script or an explicit range of a user console and return its results.
    /// A matching revision is required. Without full access, only known reads run.
    RunConsole {
        /// The console_id from get_context.
        console_id: String,
        /// The current revision of the console, from read_console or get_context.
        revision: u64,
        /// The part of the text to execute. Without it, the whole script runs.
        range: Option<Range>,
        /// Explain one statement instead of running it as is.
        explain: Option<ExplainMode>,
        /// Rows of each result in the reply. The default is 100.
        rows: Option<usize>,
    },
    /// Place SQL for the user, or edit a console. Write the SQL in the dialect of the connection.
    /// Without `console_id`, send `connection_id` and `sql`: SQLN shows the SQL in your agent tab,
    /// where the user reviews, changes, and runs it. Nothing executes. If the user changed the
    /// SQL of the tab, the new SQL waits for the user's review.
    /// With `console_id`, send `revision` and `edits`. `propose` waits for the user's review.
    /// `direct` applies the edits when the user allowed direct edits.
    EditConsole {
        /// The console to edit. Omit it to place SQL in your agent tab.
        console_id: Option<String>,
        /// With `console_id`: the current revision of the console.
        revision: Option<u64>,
        /// With `console_id`: the replacements, in any order, without overlaps.
        #[serde(default)]
        edits: Vec<Edit>,
        /// With `console_id`: `propose` waits for the user's review; `direct` needs direct edits.
        #[serde(default)]
        mode: EditMode,
        /// Without `console_id`: the connection_id of the SQL.
        connection_id: Option<String>,
        /// Without `console_id`: the schema or database of the SQL.
        schema: Option<String>,
        /// Without `console_id`: the SQL to place for the user.
        sql: Option<String>,
    },
    /// Read the catalog of a connection, or the DDL of one object.
    /// Without `object`, the reply holds the catalog: with `schema`, only that schema.
    /// A relation with `columns_loaded` false has no column list yet: describe the object.
    /// For a Kafka topic, send an `object` with the topic name and the kind `topic`; `schema`
    /// is optional. The reply lists the field paths of its Schema Registry schemas.
    /// Set `er_diagram` to read an ER diagram of `schema`, or of the `object` table.
    Describe {
        /// The connection_id from get_context.
        connection_id: String,
        /// The schema of `object`, of the diagram, or of the catalog. Optional for a Kafka topic.
        schema: Option<String>,
        /// The object whose DDL or fields to read. Omit it to read the catalog.
        object: Option<Object>,
        /// Read an ER diagram of `schema`, or of the `object` table.
        #[serde(default)]
        er_diagram: bool,
        /// Read the catalog again instead of the cached catalog.
        #[serde(default)]
        refresh: bool,
    },
    /// Compare structure or data within one SQLN instance. Never execute the sync script.
    /// A data comparison needs a table on each side, with a compatible primary key.
    Compare {
        /// `structure` compares tables and columns; `data` compares the rows of one table.
        kind: CompareKind,
        /// The side whose state the script reaches.
        source: Side,
        /// The side that the sync script changes.
        target: Side,
        /// Write DROP and DELETE in the sync script instead of comments. Nothing executes.
        #[serde(default)]
        removals: bool,
        /// Rows of the result in the reply. The default is 100.
        rows: Option<usize>,
    },
    /// Wait for a task or an edit proposal of this session, or cancel it.
    /// `artifact_offset` reads the next page of a text artifact.
    Job {
        /// The job_id of a task or proposal reply.
        job_id: String,
        /// Cancel the task instead of waiting. A proposal waits for the user only.
        #[serde(default)]
        cancel: bool,
        /// The first byte of the artifact page. Use `next_offset` of the previous page.
        #[serde(default)]
        artifact_offset: usize,
        /// Rows of each result in the reply. The default is the task's own value.
        rows: Option<usize>,
    },
}
fn page_rows() -> usize {
    100
}

impl Operation {
    /// True for operations that only read the state of SQLN.
    pub fn is_inspection(&self) -> bool {
        matches!(
            self,
            Self::RequestAccess {}
                | Self::GetContext { .. }
                | Self::ReadConsole { .. }
                | Self::ReadResult { .. }
                | Self::Job { .. }
        )
    }
}

/// Slice text without breaking a UTF-8 character. The next offset is exact.
pub fn text_page(text: &str, offset: usize) -> Reply {
    if !text.is_char_boundary(offset) {
        return Err(Error::new(
            "invalid_offset",
            "Use the next_offset from the previous page.",
        ));
    }
    let mut end = offset.saturating_add(TEXT_BYTES).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        serde_json::json!({"text": &text[offset..end], "offset": offset, "next_offset": (end < text.len()).then_some(end), "total_bytes": text.len()}),
    )
}
