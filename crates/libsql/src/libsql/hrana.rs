//! The Hrana 3 protocol over HTTP: `POST /v3/pipeline`. A stream is one SQLite
//! connection on the server. The `baton` of an answer names the stream for the next
//! request. Do not write a request or an answer in a log: a request holds the token, and
//! an answer can hold data.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::connector::http::{self, network_error};
use crate::connector::target::{NetworkTarget, Secret};
use crate::connector::{ConnectError, ErrorKind, ExecError};
use crate::text;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Execute { stmt: Stmt },
    Close,
    GetAutocommit,
}

impl Request {
    /// Runs a statement with positional arguments.
    pub fn execute(sql: &str, args: Vec<Arg>) -> Self {
        Request::Execute {
            stmt: Stmt {
                sql: sql.to_owned(),
                args,
                want_rows: true,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Stmt {
    sql: String,
    args: Vec<Arg>,
    want_rows: bool,
}

/// A value that SQLN sends.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Arg {
    Null,
    Integer { value: String },
    Text { value: String },
}

/// A value of a result row.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Cell {
    Null,
    Integer { value: String },
    Float { value: f64 },
    Text { value: String },
    Blob { base64: String },
}

impl Cell {
    /// The value as text, for the catalog reads. A blob gives its bytes as UTF-8 text.
    pub fn into_text(self) -> Option<String> {
        match self {
            Cell::Null => None,
            Cell::Integer { value } | Cell::Text { value } => Some(value),
            Cell::Float { value } => Some(format!("{value:?}")),
            Cell::Blob { base64 } => {
                Some(String::from_utf8_lossy(&decode(&base64).ok()?).into_owned())
            }
        }
    }
}

/// Decodes the bytes of a blob. The server can write the padding or not.
pub fn decode(text: &str) -> Result<Vec<u8>, ExecError> {
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    const ENGINE: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    ENGINE
        .decode(text)
        .map_err(|_| ExecError::new(text::LIBSQL_BAD_ANSWER))
}

#[derive(Debug, Deserialize)]
pub struct Col {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub decltype: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StmtResult {
    #[serde(default)]
    pub cols: Vec<Col>,
    #[serde(default)]
    pub rows: Vec<Vec<Cell>>,
    #[serde(default)]
    pub affected_row_count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Execute {
        result: StmtResult,
    },
    Close,
    GetAutocommit {
        is_autocommit: bool,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct ServerError {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub code: Option<String>,
}

impl ServerError {
    pub fn into_exec_error(self) -> ExecError {
        let kind = self.code.as_deref().map_or(ErrorKind::Other, code_kind);
        let message = match self.code {
            Some(code) if !self.message.contains(&code) => format!("{} ({code})", self.message),
            _ => self.message,
        };
        let fix = (kind == ErrorKind::Permission && message.contains("SQLITE_READONLY"))
            .then_some(crate::connector::Fix::ReadOnly);
        ExecError::with_kind(kind, message).fix(fix)
    }
}

/// The kind of a Hrana error code. A SQLite code can have an extended part, such as
/// `SQLITE_CONSTRAINT_UNIQUE`.
fn code_kind(code: &str) -> ErrorKind {
    let has = |base: &str| code == base || code.starts_with(&format!("{base}_"));
    if has("SQLITE_READONLY") || has("SQLITE_AUTH") || has("SQLITE_PERM") {
        ErrorKind::Permission
    } else if has("SQLITE_BUSY") || has("SQLITE_LOCKED") {
        ErrorKind::Timeout
    } else if has("SQLITE_FULL") || has("SQLITE_NOMEM") || has("SQLITE_TOOBIG") {
        ErrorKind::Resource
    } else if has("SQLITE_INTERRUPT") {
        ErrorKind::Canceled
    } else if matches!(
        code,
        "SQL_PARSE_ERROR" | "SQL_INPUT_ERROR" | "SQL_NO_STATEMENT"
    ) || code.starts_with("SQL_PARSE")
    {
        ErrorKind::Syntax
    } else if is_stream_code(code) {
        ErrorKind::Connection
    } else {
        ErrorKind::Other
    }
}

/// Returns true for a code of a stream that the server closed or does not know.
fn is_stream_code(code: &str) -> bool {
    code.starts_with("BATON_") || code.starts_with("STREAM_")
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Ok {
        response: Response,
    },
    Error {
        error: ServerError,
    },
    #[serde(other)]
    None,
}

#[derive(Serialize)]
struct Pipeline<'a> {
    baton: Option<&'a str>,
    requests: &'a [Request],
}

#[derive(Deserialize)]
struct PipelineAnswer {
    #[serde(default)]
    baton: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    results: Vec<Outcome>,
}

/// The error body of a refused request.
#[derive(Deserialize)]
struct RefusedAnswer {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

/// The HTTP client of one connection.
#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    token: Option<Secret>,
    /// The base address of the connection.
    pub base: String,
}

/// The stream of a session. Without a baton, the next request opens a new stream.
#[derive(Debug)]
pub struct Stream {
    pub base: String,
    pub baton: Option<String>,
}

impl Api {
    pub fn new(target: &NetworkTarget) -> Result<Self, ConnectError> {
        let (base, http) = http::endpoint(target, "libSQL", None)?;
        Ok(Self {
            http,
            token: target
                .password
                .clone()
                .filter(|token| !token.expose().is_empty()),
            base,
        })
    }

    /// Sends the requests on the stream, and keeps the baton and the address of the
    /// answer. `limit` is the most bytes of the answer.
    pub async fn pipeline(
        &self,
        stream: &mut Stream,
        requests: &[Request],
        limit: usize,
    ) -> Result<Vec<Outcome>, ExecError> {
        let body = Pipeline {
            baton: stream.baton.as_deref(),
            requests,
        };
        let mut request = self
            .http
            .post(format!("{}/v3/pipeline", stream.base))
            .json(&body);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token.expose());
        }
        let had_stream = stream.baton.is_some();
        let mut response = request.send().await.map_err(network_error)?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(part) = response.chunk().await.map_err(network_error)? {
            if bytes.len() + part.len() > limit {
                return Err(ExecError::with_kind(
                    ErrorKind::Resource,
                    text::answer_too_large("libSQL", limit),
                ));
            }
            bytes.extend_from_slice(&part);
        }
        if !(200..=299).contains(&status) {
            stream.baton = None;
            return Err(refused(status, &bytes, had_stream));
        }
        let answer: PipelineAnswer =
            serde_json::from_slice(&bytes).map_err(|_| ExecError::new(text::LIBSQL_BAD_ANSWER))?;
        stream.baton = answer.baton;
        if let Some(base) = answer.base_url
            && (base.starts_with("https://") || !stream.base.starts_with("https://"))
        {
            stream.base = base.trim_end_matches('/').to_owned();
        }
        Ok(answer.results)
    }
}

/// The error of a refused request.
fn refused(status: u16, body: &[u8], had_stream: bool) -> ExecError {
    let answer = serde_json::from_slice::<RefusedAnswer>(body).ok();
    let code = answer.as_ref().and_then(|answer| answer.code.clone());
    if matches!(status, 401 | 403) {
        return ExecError::with_kind(ErrorKind::Auth, text::LIBSQL_REFUSED);
    }
    // SQLN sends only valid requests, so a refusal on an open stream means that the
    // server does not know the stream now, as after its idle limit.
    if code.as_deref().is_some_and(is_stream_code) || (had_stream && (400..500).contains(&status)) {
        return ExecError::with_kind(ErrorKind::Connection, text::LIBSQL_STREAM_CLOSED);
    }
    match answer.and_then(|answer| answer.error) {
        Some(message) => ServerError { message, code }.into_exec_error(),
        None => ExecError::new(text::libsql_status(status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_give_their_kind() {
        let cases = [
            ("SQLITE_READONLY", ErrorKind::Permission),
            ("SQLITE_BUSY", ErrorKind::Timeout),
            ("SQLITE_CONSTRAINT_UNIQUE", ErrorKind::Other),
            ("SQL_PARSE_ERROR", ErrorKind::Syntax),
            ("SQLITE_FULL", ErrorKind::Resource),
            ("BATON_STREAM_CLOSED", ErrorKind::Connection),
            ("SQLITE_BUSYX", ErrorKind::Other),
        ];
        for (code, kind) in cases {
            assert_eq!(code_kind(code), kind, "{code}");
        }
    }

    #[test]
    fn a_refused_request_explains_its_cause() {
        assert_eq!(refused(401, b"", false).kind, ErrorKind::Auth);
        let closed = refused(400, br#"{"error":"x","code":"STREAM_EXPIRED"}"#, false);
        assert_eq!(closed.message, text::LIBSQL_STREAM_CLOSED);
        assert!(closed.closes_session());
        let closed = refused(400, b"Received an invalid baton", true);
        assert_eq!(closed.message, text::LIBSQL_STREAM_CLOSED);
        assert_eq!(
            refused(500, b"oops", false).message,
            text::libsql_status(500)
        );
    }

    #[test]
    fn a_blob_reads_with_and_without_padding() {
        assert_eq!(decode("AP8=").unwrap(), vec![0, 255]);
        assert_eq!(decode("AP8").unwrap(), vec![0, 255]);
    }
}
