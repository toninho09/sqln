//! The assistant contract. These types contain no interface or driver state.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: u32 = 2;
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
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub start: Position,
    pub end: Position,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub start: Position,
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
    pub name: String,
    pub kind: BindingKind,
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
    pub connection_id: String,
    pub schema: String,
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
    pub name: String,
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
    GetContext {
        /// The first tab to list.
        #[serde(default)]
        offset: usize,
        #[serde(default = "page_rows")]
        limit: usize,
    },
    /// Read a console text page and its revision. Offsets count UTF-8 bytes.
    /// Set `messages` to also read its Messages from record `message_offset`.
    ReadConsole {
        console_id: String,
        #[serde(default)]
        offset: usize,
        revision: Option<u64>,
        #[serde(default)]
        messages: bool,
        #[serde(default)]
        message_offset: usize,
    },
    /// Read loaded result rows without executing SQL. Reuse the returned generation for later pages.
    /// With `row` and `column`, read one large cell in blocks from `offset` bytes. Binary blocks use base64.
    ReadResult {
        tab_id: String,
        #[serde(default)]
        result: usize,
        #[serde(default)]
        offset: usize,
        #[serde(default = "page_rows")]
        limit: usize,
        generation: Option<String>,
        row: Option<usize>,
        column: Option<usize>,
    },
    /// Execute SQL on a connection in this session's agent console and return its results.
    /// Without full access, only known reads run. Use `edit_console` for SQL that the user must run.
    Query {
        connection_id: String,
        schema: Option<String>,
        sql: String,
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
        console_id: String,
        revision: u64,
        range: Option<Range>,
        explain: Option<ExplainMode>,
        rows: Option<usize>,
    },
    /// Place SQL for the user, or edit a console.
    /// Without `console_id`, send `connection_id` and `sql`: SQLN shows the SQL in your agent tab,
    /// where the user reviews, changes, and runs it. Nothing executes. If the user changed the
    /// SQL of the tab, the new SQL waits for the user's review.
    /// With `console_id`, send `revision` and `edits`. `propose` waits for the user's review.
    /// `direct` applies the edits when the user allowed direct edits.
    EditConsole {
        console_id: Option<String>,
        revision: Option<u64>,
        #[serde(default)]
        edits: Vec<Edit>,
        #[serde(default)]
        mode: EditMode,
        connection_id: Option<String>,
        schema: Option<String>,
        sql: Option<String>,
    },
    /// Read the catalog of a connection, or the DDL of one object.
    /// Set `er_diagram` to read an ER diagram of `schema`, or of the `object` table.
    Describe {
        connection_id: String,
        schema: Option<String>,
        object: Option<Object>,
        #[serde(default)]
        er_diagram: bool,
        /// Read the catalog again instead of the cached catalog.
        #[serde(default)]
        refresh: bool,
    },
    /// Compare structure or data within one SQLN instance. Never execute the sync script.
    /// A data comparison needs a table on each side, with a compatible primary key.
    Compare {
        kind: CompareKind,
        source: Side,
        target: Side,
        #[serde(default)]
        removals: bool,
        rows: Option<usize>,
    },
    /// Wait for a task or an edit proposal of this session, or cancel it.
    /// `artifact_offset` reads the next page of a text artifact.
    Job {
        job_id: String,
        #[serde(default)]
        cancel: bool,
        #[serde(default)]
        artifact_offset: usize,
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
