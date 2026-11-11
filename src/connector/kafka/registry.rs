//! The Schema Registry client. A schema id never changes its schema, so the client keeps
//! each schema for the life of the connector. Response bodies never enter messages.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use apache_avro::Schema as AvroSchema;
use prost_reflect::DescriptorPool;
use serde_json::Value as Json;
use tokio::sync::Mutex;

use crate::connector::target::{Registry, Secret, TlsFiles};
use crate::connector::tls;
use crate::ui::text;

/// The largest answer of the registry.
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
/// The deepest chain of schema references.
const REFERENCE_DEPTH: usize = 16;
/// The name of the main file of a Protobuf schema.
const MAIN_FILE: &str = "sqln-schema.proto";

/// Returns the vault account of the password of a registry. Two connections with the same
/// registry and user share the password.
pub fn account(url: &str, user: &str) -> String {
    format!(
        "kafka-schema-registry\u{1f}{}\u{1f}{user}",
        url.trim_end_matches('/')
    )
}

/// A schema that the decoder can use.
pub enum Parsed {
    Avro {
        schema: AvroSchema,
        references: Vec<AvroSchema>,
    },
    Json,
    Protobuf {
        pool: DescriptorPool,
    },
}

impl Parsed {
    pub fn kind(&self) -> &'static str {
        match self {
            Parsed::Avro { .. } => "avro",
            Parsed::Json => "json",
            Parsed::Protobuf { .. } => "protobuf",
        }
    }
}

/// The text of one schema and its references, as the registry gives them.
struct Source {
    kind: String,
    text: String,
    references: Vec<(String, String, i64)>,
}

/// Reads a password from the system vault. The connector gives the function, so tests can
/// replace the vault.
pub type VaultRead = Arc<dyn Fn(&str) -> Result<Option<Secret>, String> + Send + Sync>;

pub struct RegistryClient {
    url: String,
    user: String,
    password: Mutex<Option<Option<Secret>>>,
    vault: VaultRead,
    http: reqwest::Client,
    cache: Mutex<HashMap<u32, Arc<Parsed>>>,
}

fn http_client() -> Result<reqwest::Client, String> {
    let config = tls::client_config(&TlsFiles::default()).map_err(|error| error.message)?;
    reqwest::Client::builder()
        .tls_backend_preconfigured(config)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| text::KAFKA_REGISTRY_CLIENT.to_owned())
}

fn status_error(status: u16) -> String {
    match status {
        401 | 403 => text::KAFKA_REGISTRY_REFUSED.to_owned(),
        404 => text::KAFKA_REGISTRY_NO_SCHEMA.to_owned(),
        other => text::kafka_registry_status(other),
    }
}

impl RegistryClient {
    pub fn new(registry: &Registry, vault: VaultRead) -> Result<Self, String> {
        Ok(Self {
            url: registry.url.trim_end_matches('/').to_owned(),
            user: registry.user.clone(),
            password: Mutex::new(registry.password.clone().map(Some)),
            vault,
            http: http_client()?,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Returns the password of the registry. The vault read happens one time.
    async fn password(&self) -> Result<Option<Secret>, String> {
        if self.user.is_empty() {
            return Ok(None);
        }
        let mut password = self.password.lock().await;
        if let Some(known) = password.as_ref() {
            return Ok(known.clone());
        }
        let vault = self.vault.clone();
        let account = account(&self.url, &self.user);
        let read = tokio::task::spawn_blocking(move || vault(&account))
            .await
            .map_err(|_| text::KAFKA_REGISTRY_VAULT.to_owned())??;
        *password = Some(read.clone());
        Ok(read)
    }

    async fn get(&self, path: &str) -> Result<Json, String> {
        let mut request = self
            .http
            .get(format!("{}{path}", self.url))
            .header("Accept", "application/vnd.schemaregistry.v1+json");
        if !self.user.is_empty() {
            let password = self.password().await?;
            request = request.basic_auth(&self.user, password.as_ref().map(Secret::expose));
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| text::KAFKA_REGISTRY_UNREACHABLE.to_owned())?;
        if !response.status().is_success() {
            return Err(status_error(response.status().as_u16()));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| text::KAFKA_REGISTRY_UNREACHABLE.to_owned())?
        {
            if body.len() + chunk.len() > RESPONSE_LIMIT {
                return Err(text::KAFKA_REGISTRY_BAD_ANSWER.to_owned());
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| text::KAFKA_REGISTRY_BAD_ANSWER.to_owned())
    }

    fn source(answer: &Json) -> Result<Source, String> {
        let bad = || text::KAFKA_REGISTRY_BAD_ANSWER.to_owned();
        let text = answer
            .get("schema")
            .and_then(Json::as_str)
            .ok_or_else(bad)?;
        let kind = answer
            .get("schemaType")
            .and_then(Json::as_str)
            .unwrap_or("AVRO")
            .to_ascii_uppercase();
        let mut references = Vec::new();
        for reference in answer
            .get("references")
            .and_then(Json::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            let name = reference
                .get("name")
                .and_then(Json::as_str)
                .ok_or_else(bad)?;
            let subject = reference
                .get("subject")
                .and_then(Json::as_str)
                .ok_or_else(bad)?;
            let version = reference
                .get("version")
                .and_then(Json::as_i64)
                .ok_or_else(bad)?;
            references.push((name.to_owned(), subject.to_owned(), version));
        }
        Ok(Source {
            kind,
            text: text.to_owned(),
            references,
        })
    }

    /// Reads the references of a schema, depth first, so each schema comes after the
    /// schemas that it uses. It returns each reference with its import name.
    async fn references(&self, source: &Source) -> Result<Vec<(String, String)>, String> {
        enum Step {
            Visit((String, String, i64), usize),
            Emit(String, String),
        }
        let mut found = Vec::new();
        let mut seen: Vec<(String, i64)> = Vec::new();
        let mut stack: Vec<Step> = source
            .references
            .iter()
            .rev()
            .map(|reference| Step::Visit(reference.clone(), 0))
            .collect();
        while let Some(step) = stack.pop() {
            let ((name, subject, version), depth) = match step {
                Step::Emit(name, text) => {
                    found.push((name, text));
                    continue;
                }
                Step::Visit(reference, depth) => (reference, depth),
            };
            if depth > REFERENCE_DEPTH {
                return Err(text::KAFKA_REGISTRY_DEEP.to_owned());
            }
            if seen.contains(&(subject.clone(), version)) {
                continue;
            }
            seen.push((subject.clone(), version));
            let path = format!("/subjects/{}/versions/{version}", percent_encode(&subject));
            let inner = Self::source(&self.get(&path).await?)?;
            stack.push(Step::Emit(name, inner.text));
            for reference in inner.references.into_iter().rev() {
                stack.push(Step::Visit(reference, depth + 1));
            }
        }
        Ok(found)
    }

    /// Returns the schema of an id. A failure stays out of the cache, so a later message
    /// asks again.
    pub async fn schema(&self, id: u32) -> Result<Arc<Parsed>, String> {
        if let Some(parsed) = self.cache.lock().await.get(&id) {
            return Ok(parsed.clone());
        }
        let answer = self.get(&format!("/schemas/ids/{id}")).await?;
        let source = Self::source(&answer)?;
        let references = self.references(&source).await?;
        let parsed = Arc::new(parse(&source.kind, &source.text, &references)?);
        self.cache.lock().await.insert(id, parsed.clone());
        Ok(parsed)
    }
}

/// Encodes a subject for a path. A subject can hold any character.
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Gives the sources of the references to the Protobuf compiler, by import name.
struct Sources(HashMap<String, String>);

impl protox::file::FileResolver for Sources {
    fn resolve_path(&self, path: &std::path::Path) -> Option<String> {
        let name = path.to_str()?;
        self.0.contains_key(name).then(|| name.to_owned())
    }

    fn open_file(&self, name: &str) -> Result<protox::file::File, protox::Error> {
        match self.0.get(name) {
            Some(source) => protox::file::File::from_source(name, source),
            None => Err(protox::Error::file_not_found(name)),
        }
    }
}

/// Parses the text of a schema with its references.
pub fn parse(kind: &str, text: &str, references: &[(String, String)]) -> Result<Parsed, String> {
    match kind {
        "AVRO" => {
            let (schema, references) = AvroSchema::parse_str_with_list(
                text,
                references.iter().map(|(_, source)| source.as_str()),
            )
            .map_err(|_| text::KAFKA_REGISTRY_BAD_SCHEMA.to_owned())?;
            Ok(Parsed::Avro { schema, references })
        }
        "JSON" => Ok(Parsed::Json),
        "PROTOBUF" => {
            let mut files: HashMap<String, String> = references.iter().cloned().collect();
            files.insert(MAIN_FILE.to_owned(), text.to_owned());
            let mut resolver = protox::file::ChainFileResolver::new();
            resolver.add(Sources(files));
            resolver.add(protox::file::GoogleFileResolver::new());
            let mut compiler = protox::Compiler::with_file_resolver(resolver);
            compiler
                .open_file(MAIN_FILE)
                .map_err(|_| text::KAFKA_REGISTRY_BAD_SCHEMA.to_owned())?;
            Ok(Parsed::Protobuf {
                pool: compiler.descriptor_pool(),
            })
        }
        _ => Err(text::KAFKA_REGISTRY_BAD_SCHEMA.to_owned()),
    }
}

/// The name of the main file of a Protobuf schema, for the decoder.
pub fn main_file() -> &'static str {
    MAIN_FILE
}
