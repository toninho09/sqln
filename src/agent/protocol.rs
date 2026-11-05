//! The assistant contract. These types contain no interface or driver state.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: u32 = 1;
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SqlTemplate {
    SelectAll,
    SelectColumns,
    Insert,
    Update,
    Delete,
}
impl SqlTemplate {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SelectAll => "select_all",
            Self::SelectColumns => "select_columns",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

/// Each variant becomes one MCP tool. Fields are deliberately explicit.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Request access in the SQLN interface. The user must approve this session.
    RequestAccess {},
    /// Read the authorization state of this session.
    GetAccess {},
    /// Read the active tab and the available assistant capabilities.
    GetContext {},
    /// List existing connection labels, dialects, and capabilities. No secrets.
    ListConnections {
        #[serde(default)]
        offset: usize,
        #[serde(default = "page_rows")]
        limit: usize,
    },
    /// List open consoles, grids, diagrams, and comparisons.
    ListTabs {
        #[serde(default)]
        offset: usize,
        #[serde(default = "page_rows")]
        limit: usize,
    },
    /// Read a console text page and its revision. Offsets count UTF-8 bytes.
    ReadConsole {
        console_id: String,
        #[serde(default)]
        offset: usize,
        revision: Option<u64>,
    },
    /// Read a page of console messages. The offset counts records.
    ReadMessages {
        console_id: String,
        #[serde(default)]
        offset: usize,
        #[serde(default)]
        text_offset: usize,
    },
    /// Read loaded result rows without executing SQL. Reuse the returned generation for later pages.
    ReadResult {
        tab_id: String,
        #[serde(default)]
        result: usize,
        #[serde(default)]
        offset: usize,
        #[serde(default = "page_rows")]
        limit: usize,
        generation: Option<String>,
    },
    /// Read one large cell in blocks. Binary blocks use base64.
    ReadResultCell {
        tab_id: String,
        #[serde(default)]
        result: usize,
        row: usize,
        column: usize,
        generation: String,
        #[serde(default)]
        offset: usize,
    },
    /// Open an assistant console without changing focus. Reuses this session's console for the connection.
    OpenAgentConsole {
        connection_id: String,
        schema: Option<String>,
    },
    /// Execute new read-only SQL in this session's own console. Does not change editor text.
    RunReadQuery {
        console_id: String,
        sql: String,
        #[serde(default)]
        parameters: Vec<Binding>,
    },
    /// Execute the current script or an explicit range in a console. A matching revision is required.
    RunConsole {
        console_id: String,
        revision: u64,
        range: Option<Range>,
    },
    /// Read this session's task state and result references.
    GetJob { job_id: String },
    /// Cancel this session's task, never another session's task.
    CancelJob { job_id: String },
    /// Propose edits, or apply them only if the user enabled direct editing for this console.
    EditConsole {
        console_id: String,
        revision: u64,
        edits: Vec<Edit>,
        #[serde(default)]
        mode: EditMode,
    },
    /// Read this session's edit proposal state. Only the user can accept it.
    GetProposal { proposal_id: String },
    /// Read the cached catalog or request an authorized read when refresh is true.
    GetCatalog {
        connection_id: String,
        #[serde(default)]
        refresh: bool,
    },
    /// Read the database definition of a table, view, routine, sequence, trigger, or type.
    GetObjectDdl {
        connection_id: String,
        schema: String,
        name: String,
        kind: DdlKind,
        identity: Option<String>,
    },
    /// Generate SQL from the existing templates. This tool never executes the output.
    GenerateSql {
        connection_id: String,
        schema: String,
        table: String,
        template: SqlTemplate,
    },
    /// Explain one statement from a console. Analyze executes only an approved read.
    Explain {
        console_id: String,
        revision: u64,
        range: Option<Range>,
        #[serde(default)]
        analyze: bool,
    },
    /// Read an ER diagram as a visible background tab and Mermaid text.
    GetErDiagram {
        connection_id: String,
        schema: String,
        table: Option<String>,
    },
    /// Compare structure within one SQLN instance. Never execute the sync script.
    CompareSchema {
        source: Side,
        target: Side,
        #[serde(default)]
        removals: bool,
    },
    /// Compare data within one SQLN instance. Both tables must have a compatible primary key.
    CompareData {
        source: Side,
        target: Side,
        #[serde(default)]
        removals: bool,
    },
    /// Read a text artifact in blocks. Offsets count UTF-8 bytes.
    ReadArtifact {
        job_id: String,
        #[serde(default)]
        offset: usize,
    },
}
fn page_rows() -> usize {
    100
}

impl Operation {
    pub fn is_inspection(&self) -> bool {
        matches!(
            self,
            Self::RequestAccess {}
                | Self::GetAccess {}
                | Self::GetContext {}
                | Self::ListConnections { .. }
                | Self::ListTabs { .. }
                | Self::ReadConsole { .. }
                | Self::ReadMessages { .. }
                | Self::ReadResult { .. }
                | Self::ReadResultCell { .. }
                | Self::GetJob { .. }
                | Self::GetProposal { .. }
                | Self::ReadArtifact { .. }
                | Self::CancelJob { .. }
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
