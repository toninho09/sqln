//! The requests of the SQL Statement Execution API 2.0. Do not write a request or an
//! answer in a log: a request holds the token, and an answer can hold data.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::connector::http::{self, network_error};
use crate::connector::target::{NetworkTarget, Secret, TlsFiles};
use crate::connector::{ConnectError, ErrorKind, ExecError, describe};
use crate::text;

/// The time that the first request waits for the end of a statement. The API accepts
/// `0s` or 5 to 50 seconds. A cancel that arrives in this time waits for the answer.
const WAIT: &str = "5s";
/// The time limit of one API request. The first request waits `WAIT` at most.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// The time limit between two parts of a download.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Serialize)]
struct StatementRequest<'a> {
    statement: &'a str,
    warehouse_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<&'a str>,
    disposition: &'static str,
    format: &'static str,
    wait_timeout: &'static str,
    on_wait_timeout: &'static str,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    Pending,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Closed,
}

impl State {
    pub fn is_final(self) -> bool {
        !matches!(self, State::Pending | State::Running)
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

impl ApiError {
    pub fn into_exec_error(self) -> ExecError {
        let message = self
            .message
            .unwrap_or_else(|| text::DATABRICKS_BAD_ANSWER.to_owned());
        let kind = self
            .error_code
            .as_deref()
            .map_or(ErrorKind::Other, code_kind);
        ExecError::with_kind(
            kind,
            match self.error_code {
                Some(code) => format!("{message} ({code})"),
                None => message,
            },
        )
    }
}

/// The kind of an error code of the API. See "Error handling" of the Databricks REST API.
fn code_kind(code: &str) -> ErrorKind {
    match code {
        "PARSE_SYNTAX_ERROR"
        | "TABLE_OR_VIEW_NOT_FOUND"
        | "UNRESOLVED_COLUMN"
        | "UNRESOLVED_ROUTINE"
        | "SCHEMA_NOT_FOUND"
        | "INVALID_PARAMETER_VALUE"
        | "BAD_REQUEST" => ErrorKind::Syntax,
        "PERMISSION_DENIED" | "INSUFFICIENT_PERMISSIONS" => ErrorKind::Permission,
        "UNAUTHENTICATED" => ErrorKind::Auth,
        "DEADLINE_EXCEEDED" => ErrorKind::Timeout,
        "CANCELLED" | "ABORTED" => ErrorKind::Canceled,
        "RESOURCE_EXHAUSTED" | "REQUEST_LIMIT_EXCEEDED" | "TEMPORARILY_UNAVAILABLE" => {
            ErrorKind::Resource
        }
        "NOT_IMPLEMENTED" | "FEATURE_DISABLED" => ErrorKind::Unsupported,
        _ => ErrorKind::Other,
    }
}

/// The kind of an HTTP status without an error code of the API.
fn status_kind(status: u16) -> ErrorKind {
    match status {
        401 | 403 => ErrorKind::Auth,
        408 | 504 => ErrorKind::Timeout,
        429 | 503 => ErrorKind::Resource,
        501 => ErrorKind::Unsupported,
        _ => ErrorKind::Other,
    }
}

#[derive(Debug, Deserialize)]
pub struct Status {
    pub state: State,
    #[serde(default)]
    pub error: Option<ApiError>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Column {
    pub name: String,
    /// The SQL type, such as `DECIMAL(10,2)`. Older answers can omit it.
    #[serde(default)]
    pub type_text: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ResultSchema {
    #[serde(default)]
    pub columns: Vec<Column>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub schema: ResultSchema,
    #[serde(default)]
    pub total_chunk_count: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExternalLink {
    pub chunk_index: usize,
    pub external_link: String,
    /// Some clouds need these headers on the download.
    #[serde(default)]
    pub http_headers: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ResultData {
    #[serde(default)]
    pub external_links: Vec<ExternalLink>,
}

#[derive(Debug, Deserialize)]
pub struct Answer {
    pub statement_id: String,
    pub status: Status,
    #[serde(default)]
    pub manifest: Option<Manifest>,
    #[serde(default)]
    pub result: Option<ResultData>,
}

/// The HTTP clients of one connection. Clones share the connection pools of `reqwest`.
#[derive(Clone)]
pub struct Api {
    base: String,
    warehouse: String,
    token: Secret,
    http: reqwest::Client,
    /// The client of the result links. It never sends the token, because a link names
    /// cloud storage and not the workspace.
    download: reqwest::Client,
    /// False only for a test server without TLS. A link without TLS is then accepted.
    tls: bool,
}

/// Refuses a statement id that is not one path segment.
fn segment(id: &str) -> Result<&str, ExecError> {
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Ok(id)
    } else {
        Err(ExecError::new(text::DATABRICKS_BAD_ANSWER))
    }
}

impl Api {
    pub fn new(target: &NetworkTarget) -> Result<Self, ConnectError> {
        let token = target
            .password
            .clone()
            .filter(|token| !token.expose().is_empty())
            .ok_or_else(|| ConnectError::refused_password(text::DATABRICKS_NO_TOKEN))?;
        let (base, http) = http::endpoint(target, "Databricks", Some(READ_TIMEOUT))?;
        Ok(Self {
            tls: base.starts_with("https://"),
            base: format!("{base}/api/2.0/sql"),
            warehouse: target.databricks.warehouse.clone(),
            token,
            http,
            download: http::client(Some(&TlsFiles::default()), Some(READ_TIMEOUT))?,
        })
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{}{path}", self.base))
            .bearer_auth(self.token.expose())
            .timeout(REQUEST_TIMEOUT)
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{}{path}", self.base))
            .bearer_auth(self.token.expose())
            .timeout(REQUEST_TIMEOUT)
    }

    /// Checks the token and the warehouse.
    pub async fn check(&self) -> Result<(), ConnectError> {
        let response = self
            .get(&format!("/warehouses/{}", self.warehouse))
            .send()
            .await
            .map_err(|error| ConnectError::new(describe(&error.without_url())))?;
        match response.status().as_u16() {
            200..=299 => Ok(()),
            401 | 403 => Err(ConnectError::refused_password(text::DATABRICKS_REFUSED)),
            404 => Err(ConnectError::new(text::DATABRICKS_NO_SUCH_WAREHOUSE)),
            status => Err(ConnectError::new(text::databricks_status(status))),
        }
    }

    /// Reads the JSON answer of a request. A refused request gives the error of the API.
    async fn answer<T: serde::de::DeserializeOwned>(
        request: reqwest::RequestBuilder,
    ) -> Result<T, ExecError> {
        let response = request.send().await.map_err(network_error)?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(network_error)?;
        if !(200..=299).contains(&status) {
            return Err(match serde_json::from_slice::<ApiError>(&body) {
                Ok(error) if error.message.is_some() => error.into_exec_error(),
                _ if matches!(status, 401 | 403) => {
                    ExecError::with_kind(ErrorKind::Auth, text::DATABRICKS_REFUSED)
                }
                _ => ExecError::with_kind(status_kind(status), text::databricks_status(status)),
            });
        }
        serde_json::from_slice(&body).map_err(|_| ExecError::new(text::DATABRICKS_BAD_ANSWER))
    }

    pub async fn submit(
        &self,
        sql: &str,
        catalog: Option<&str>,
        schema: Option<&str>,
    ) -> Result<Answer, ExecError> {
        let request = StatementRequest {
            statement: sql,
            warehouse_id: &self.warehouse,
            catalog,
            schema,
            disposition: "EXTERNAL_LINKS",
            format: "ARROW_STREAM",
            wait_timeout: WAIT,
            on_wait_timeout: "CONTINUE",
        };
        Self::answer(self.post("/statements/").json(&request)).await
    }

    pub async fn status(&self, id: &str) -> Result<Answer, ExecError> {
        Self::answer(self.get(&format!("/statements/{}", segment(id)?))).await
    }

    pub async fn chunk(&self, id: &str, index: usize) -> Result<ResultData, ExecError> {
        Self::answer(self.get(&format!(
            "/statements/{}/result/chunks/{index}",
            segment(id)?
        )))
        .await
    }

    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        let id = segment(id).map_err(|error| error.message)?;
        let response = self
            .post(&format!("/statements/{id}/cancel"))
            .send()
            .await
            .map_err(|error| describe(&error.without_url()))?;
        match response.status().as_u16() {
            200..=299 => Ok(()),
            status => Err(text::databricks_status(status)),
        }
    }

    /// Downloads one result part, up to `limit` bytes. `None` means that the link is not
    /// valid now, as after its expiry.
    pub async fn download(
        &self,
        link: &ExternalLink,
        limit: usize,
    ) -> Result<Option<Vec<u8>>, ExecError> {
        if self.tls && !link.external_link.starts_with("https://") {
            return Err(ExecError::new(text::DATABRICKS_UNSAFE_LINK));
        }
        let mut request = self.download.get(&link.external_link);
        for (name, value) in &link.http_headers {
            request = request.header(name, value);
        }
        let mut response = request.send().await.map_err(network_error)?;
        match response.status().as_u16() {
            200..=299 => {}
            400 | 403 | 404 | 410 => return Ok(None),
            status => return Err(ExecError::new(text::databricks_status(status))),
        }
        let mut bytes = Vec::new();
        while let Some(part) = response.chunk().await.map_err(network_error)? {
            if bytes.len() + part.len() > limit {
                let size = response
                    .content_length()
                    .unwrap_or((bytes.len() + part.len()) as u64);
                return Err(ExecError::new(text::databricks_chunk_too_large(
                    size, limit,
                )));
            }
            bytes.extend_from_slice(&part);
        }
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_codes_and_statuses_give_their_kind() {
        let error = |code: &str| ApiError {
            error_code: Some(code.to_owned()),
            message: Some("refused".to_owned()),
        };
        assert_eq!(
            error("PERMISSION_DENIED").into_exec_error().kind,
            ErrorKind::Permission
        );
        assert_eq!(
            error("PARSE_SYNTAX_ERROR").into_exec_error().kind,
            ErrorKind::Syntax
        );
        assert_eq!(
            error("SOMETHING_NEW").into_exec_error().kind,
            ErrorKind::Other
        );
        assert_eq!(status_kind(401), ErrorKind::Auth);
        assert_eq!(status_kind(429), ErrorKind::Resource);
        assert_eq!(status_kind(500), ErrorKind::Other);
    }
}
