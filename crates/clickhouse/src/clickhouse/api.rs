//! The HTTP interface of ClickHouse. Do not write a request or an answer in a log: a
//! request holds the password, and an answer can hold data.

use crate::connector::http::{self, network_error};
use crate::connector::target::{NetworkTarget, Secret};
use crate::connector::{ConnectError, ErrorKind, ExecError};
use crate::text;

/// The output format of each statement without its own `FORMAT` clause.
pub const FORMAT: &str = "TabSeparatedWithNamesAndTypes";
/// The header with the output format of the answer.
const FORMAT_HEADER: &str = "x-clickhouse-format";
/// The header with the error code of a refused statement.
const CODE_HEADER: &str = "x-clickhouse-exception-code";
/// The line that starts an error in the middle of an answer, in ClickHouse 24.11 and
/// newer.
const EXCEPTION_MARK: &[u8] = b"__exception__";

/// The HTTP client of one connection. Clones share the connection pool of `reqwest`.
#[derive(Clone)]
pub struct Api {
    base: reqwest::Url,
    http: reqwest::Client,
    user: String,
    password: Option<Secret>,
}

impl Api {
    pub fn new(target: &NetworkTarget) -> Result<Self, ConnectError> {
        let (base, http) = http::endpoint(target, "ClickHouse", None)?;
        let base = reqwest::Url::parse(&format!("{base}/"))
            .map_err(|_| ConnectError::new("The ClickHouse host is not valid."))?;
        Ok(Self {
            base,
            http,
            user: target.user.clone(),
            password: target.password.clone(),
        })
    }

    /// Sends a statement with the query parameters of the URL. A refused statement
    /// gives the error of ClickHouse.
    pub async fn send(
        &self,
        sql: &str,
        parameters: &[(&str, &str)],
    ) -> Result<reqwest::Response, ExecError> {
        let mut url = self.base.clone();
        if !parameters.is_empty() {
            url.query_pairs_mut().extend_pairs(parameters);
        }
        let mut request = self
            .http
            .post(url)
            .header("X-ClickHouse-User", &self.user)
            .body(sql.to_owned());
        if let Some(password) = &self.password {
            request = request.header("X-ClickHouse-Key", password.expose());
        }
        let response = request.send().await.map_err(network_error)?;
        let status = response.status().as_u16();
        if (200..=299).contains(&status) {
            return Ok(response);
        }
        let code = response
            .headers()
            .get(CODE_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u32>().ok());
        let body = response.bytes().await.map_err(network_error)?;
        let message = String::from_utf8_lossy(&body);
        let message = message.trim();
        if message.is_empty() {
            let kind = if matches!(status, 401 | 403) {
                ErrorKind::Auth
            } else {
                ErrorKind::Other
            };
            return Err(ExecError::with_kind(kind, text::clickhouse_status(status)));
        }
        Err(server_error(message, code))
    }

    /// Stops a statement with its query id. The request has no session, because
    /// ClickHouse runs one request of a session at a time.
    pub async fn kill(&self, query_id: &str) -> Result<(), String> {
        let sql = format!("KILL QUERY WHERE query_id = '{query_id}' ASYNC");
        self.send(&sql, &[])
            .await
            .map(drop)
            .map_err(|error| error.message)
    }
}

/// Returns true when the answer uses `FORMAT`. A statement with its own `FORMAT` clause
/// gets the format of the clause.
pub fn uses_format(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(FORMAT_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|format| format == FORMAT)
}

/// The error of a ClickHouse text, such as `Code: 62. DB::Exception: Syntax error: failed
/// at position 8 ...`. The code of the header wins over the code of the text.
pub fn server_error(message: &str, code: Option<u32>) -> ExecError {
    let code = code.or_else(|| {
        let rest = message.strip_prefix("Code: ")?;
        rest[..rest.find(|c: char| !c.is_ascii_digit())?]
            .parse()
            .ok()
    });
    let offset = message
        .split_once("failed at position ")
        .and_then(|(_, rest)| {
            let digits = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            rest[..digits].parse::<usize>().ok()
        })
        .and_then(|position| position.checked_sub(1));
    ExecError::with_kind(code.map_or(ErrorKind::Other, code_kind), message).at(offset)
}

/// Returns true when a password or a user refusal gave the error.
pub fn is_refusal(error: &ExecError) -> bool {
    error.kind == ErrorKind::Auth
}

/// The kind of a ClickHouse error code. See `src/Common/ErrorCodes.cpp` of ClickHouse.
fn code_kind(code: u32) -> ErrorKind {
    match code {
        // SYNTAX_ERROR, UNKNOWN_IDENTIFIER, UNKNOWN_FUNCTION, UNKNOWN_TABLE,
        // UNKNOWN_DATABASE, TYPE_MISMATCH, ILLEGAL_TYPE_OF_ARGUMENT.
        62 | 47 | 46 | 60 | 81 | 53 | 43 => ErrorKind::Syntax,
        // READONLY, ACCESS_DENIED.
        164 | 497 => ErrorKind::Permission,
        // UNKNOWN_USER, WRONG_PASSWORD, REQUIRED_PASSWORD, AUTHENTICATION_FAILED.
        192..=194 | 516 => ErrorKind::Auth,
        // TIMEOUT_EXCEEDED.
        159 => ErrorKind::Timeout,
        // QUERY_WAS_CANCELLED.
        394 => ErrorKind::Canceled,
        // MEMORY_LIMIT_EXCEEDED, TOO_MANY_SIMULTANEOUS_QUERIES, NOT_ENOUGH_SPACE.
        241 | 202 | 243 => ErrorKind::Resource,
        // NOT_IMPLEMENTED, SUPPORT_IS_DISABLED.
        48 | 344 => ErrorKind::Unsupported,
        _ => ErrorKind::Other,
    }
}

/// Reads the lines of an answer, one part at a time.
pub struct Lines {
    response: reqwest::Response,
    buffer: Vec<u8>,
    /// The start of the first line in `buffer` that the reader did not return.
    start: usize,
    done: bool,
    limit: usize,
}

impl Lines {
    /// `limit` is the most bytes of one line.
    pub fn new(response: reqwest::Response, limit: usize) -> Self {
        Self {
            response,
            buffer: Vec::new(),
            start: 0,
            done: false,
            limit,
        }
    }

    /// Returns the next line, without its line feed. An error that ClickHouse writes in
    /// the middle of the answer gives its `ExecError`.
    pub async fn next(&mut self) -> Result<Option<Vec<u8>>, ExecError> {
        let line = self.next_raw().await?;
        match line {
            Some(line) if line == EXCEPTION_MARK => {
                let mut message = Vec::new();
                while let Some(part) = self.next_raw().await? {
                    message.extend_from_slice(&part);
                    message.push(b'\n');
                }
                Err(stream_error(&message))
            }
            Some(line) if line.starts_with(b"Code: ") && contains(&line, b"DB::Exception") => {
                Err(stream_error(&line))
            }
            line => Ok(line),
        }
    }

    async fn next_raw(&mut self) -> Result<Option<Vec<u8>>, ExecError> {
        loop {
            if let Some(end) = self.buffer[self.start..].iter().position(|b| *b == b'\n') {
                let line = self.buffer[self.start..self.start + end].to_vec();
                self.start += end + 1;
                return Ok(Some(line));
            }
            if self.done {
                if self.start == self.buffer.len() {
                    return Ok(None);
                }
                let line = self.buffer[self.start..].to_vec();
                self.start = self.buffer.len();
                return Ok(Some(line));
            }
            self.buffer.drain(..self.start);
            self.start = 0;
            match self.response.chunk().await.map_err(network_error)? {
                Some(part) => {
                    if self.buffer.len() + part.len() > self.limit {
                        return Err(ExecError::with_kind(
                            ErrorKind::Resource,
                            text::answer_too_large("ClickHouse", self.limit),
                        ));
                    }
                    self.buffer.extend_from_slice(&part);
                }
                None => self.done = true,
            }
        }
    }
}

fn contains(bytes: &[u8], part: &[u8]) -> bool {
    bytes.windows(part.len()).any(|window| window == part)
}

fn stream_error(message: &[u8]) -> ExecError {
    let message = String::from_utf8_lossy(message);
    let message = message.trim();
    // The text after the mark can start with the tag of the exception.
    let message = message
        .find("Code: ")
        .map_or(message, |start| &message[start..]);
    server_error(message, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_text_gives_its_kind_and_position() {
        let error = server_error(
            "Code: 62. DB::Exception: Syntax error: failed at position 8 ('FROMM'): \
             FROMM t. (SYNTAX_ERROR) (version 24.8.1.1)",
            None,
        );
        assert_eq!(error.kind, ErrorKind::Syntax);
        assert_eq!(error.offset, Some(7));
        let error = server_error(
            "Code: 516. DB::Exception: default: Authentication failed",
            None,
        );
        assert!(is_refusal(&error));
        let error = server_error("Code: 1. DB::Exception: other", Some(394));
        assert_eq!(error.kind, ErrorKind::Canceled);
        assert_eq!(server_error("no code", None).kind, ErrorKind::Other);
    }
}
