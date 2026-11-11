//! The configuration of librdkafka. TLS follows the rule of SQLN, and the roots are the
//! roots of the other connectors. librdkafka keeps its own logs out of SQLN.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use rdkafka::client::ClientContext;
use rdkafka::config::{ClientConfig, RDKafkaLogLevel};
use rdkafka::consumer::{BaseConsumer, Consumer as _, ConsumerContext, StreamConsumer};
use rdkafka::error::{KafkaError, RDKafkaErrorCode};

use crate::connector::target::{Address, NetworkTarget, Tls};
use crate::connector::{ConnectError, ExecError, describe, tls};
use crate::ui::text;

/// A call to the cluster that gets no answer in this time fails.
pub const CALL_LIMIT: Duration = Duration::from_secs(10);

/// The group of the reads of SQLN. librdkafka needs a group for a consumer, but SQLN never
/// joins it and never commits, so the cluster never creates it.
pub const READ_GROUP: &str = "sqln-read";

/// Keeps the last error of the client, so a failed call can give its reason. The logs of
/// librdkafka can hold names and values, so they go nowhere.
#[derive(Default)]
pub struct Context {
    last_error: Mutex<Option<String>>,
    /// A refused password. Later errors, such as "all brokers down", do not hide it.
    refusal: Mutex<Option<String>>,
}

impl Context {
    fn take(slot: &Mutex<Option<String>>) -> Option<String> {
        slot.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

impl ClientContext for Context {
    fn log(&self, _level: RDKafkaLogLevel, _facility: &str, _message: &str) {}

    fn error(&self, error: KafkaError, reason: &str) {
        let slot = if is_refusal(error.rdkafka_error_code()) {
            &self.refusal
        } else {
            &self.last_error
        };
        *slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(reason.to_owned());
    }
}

impl ConsumerContext for Context {}

pub type Consumer = BaseConsumer<Context>;
pub type Reader = StreamConsumer<Context>;

/// Returns the roots of the system in PEM form, for librdkafka. The bundled roots of the
/// other connectors have no certificate form, so only the system roots go here.
fn roots_pem() -> &'static str {
    static PEM: OnceLock<String> = OnceLock::new();
    PEM.get_or_init(|| {
        let engine = base64::engine::general_purpose::STANDARD;
        let mut pem = String::new();
        for root in tls::system_roots() {
            pem.push_str("-----BEGIN CERTIFICATE-----\n");
            let encoded = engine.encode(root.as_ref());
            for line in encoded.as_bytes().chunks(64) {
                pem.push_str(std::str::from_utf8(line).unwrap_or_default());
                pem.push('\n');
            }
            pem.push_str("-----END CERTIFICATE-----\n");
        }
        pem
    })
}

fn path_text(path: &std::path::Path) -> Result<String, ConnectError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ConnectError::new(text::KAFKA_PATH_TEXT))
}

/// Builds the configuration of a consumer of the target, in the group `group`.
pub fn config(target: &NetworkTarget, group: &str) -> Result<ClientConfig, ConnectError> {
    let Address::Tcp { host, port } = &target.address else {
        return Err(ConnectError::new(text::FORM_KAFKA_SOCKET));
    };
    let place = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut config = ClientConfig::new();
    config
        .set("bootstrap.servers", place)
        .set("client.id", "sqln")
        .set("group.id", group)
        .set("enable.auto.commit", "false")
        .set("enable.auto.offset.store", "false")
        .set("enable.partition.eof", "true")
        .set("auto.offset.reset", "earliest")
        .set("socket.connection.setup.timeout.ms", "10000")
        .set("allow.auto.create.topics", "false")
        .set_log_level(RDKafkaLogLevel::Emerg);
    let sasl = !target.user.is_empty();
    let protocol = match (&target.tls, sasl) {
        (Tls::Disabled, false) => "plaintext",
        (Tls::Disabled, true) => "sasl_plaintext",
        (Tls::Verify(_), false) => "ssl",
        (Tls::Verify(_), true) => "sasl_ssl",
    };
    config.set("security.protocol", protocol);
    if sasl {
        config
            .set("sasl.mechanism", target.kafka.sasl.name())
            .set("sasl.username", &target.user)
            .set(
                "sasl.password",
                target
                    .password
                    .as_ref()
                    .map_or("", |password| password.expose()),
            );
    }
    if let Tls::Verify(files) = &target.tls {
        config.set("ssl.endpoint.identification.algorithm", "https");
        match &files.ca {
            Some(ca) => config.set("ssl.ca.location", path_text(ca)?),
            None if tls::system_roots().is_empty() => {
                return Err(ConnectError::new(text::KAFKA_NO_ROOTS));
            }
            None => config.set("ssl.ca.pem", roots_pem()),
        };
        if let Some(client) = &files.client {
            config
                .set("ssl.certificate.location", path_text(&client.certificate)?)
                .set("ssl.key.location", path_text(&client.key)?);
        }
    }
    Ok(config)
}

/// Creates a consumer. Creation reads no network, so it does not wait.
pub fn consumer(config: &ClientConfig) -> Result<Consumer, ConnectError> {
    config
        .create_with_context(Context::default())
        .map_err(|error| ConnectError::new(describe(&error)))
}

pub fn reader(config: &ClientConfig) -> Result<Reader, ExecError> {
    config
        .create_with_context(Context::default())
        .map_err(|error| ExecError::new(describe(&error)))
}

fn is_refusal(code: Option<RDKafkaErrorCode>) -> bool {
    matches!(
        code,
        Some(RDKafkaErrorCode::Authentication | RDKafkaErrorCode::SaslAuthenticationFailed)
    )
}

/// Describes a failed call. The last error of the client often holds the real reason,
/// such as a refused password, when the call only reports a time limit.
pub fn call_error(consumer: &Consumer, error: &KafkaError) -> (bool, String) {
    let context = consumer.context();
    let mut refusal = Context::take(&context.refusal);
    let mut last = Context::take(&context.last_error);
    // A consumer receives the errors of its brokers as events of its queue. A consumer
    // without partitions has only these events in the queue.
    for _ in 0..32 {
        match consumer.poll(Duration::ZERO) {
            Some(Err(event)) if is_refusal(event.rdkafka_error_code()) => {
                refusal.get_or_insert_with(|| describe(&event));
            }
            Some(Err(event)) => {
                last.get_or_insert_with(|| describe(&event));
            }
            Some(Ok(_)) => {}
            None => break,
        }
    }
    let refused = refusal.is_some() || is_refusal(error.rdkafka_error_code());
    let text = match refusal.or(last) {
        Some(reason) if !describe(error).contains(&reason) => {
            format!("{}: {reason}", describe(error))
        }
        _ => describe(error),
    };
    (refused, text)
}

/// Runs a call of librdkafka that waits, on a thread for blocking work.
pub async fn blocking<T: Send + 'static>(
    consumer: &Arc<Consumer>,
    call: impl FnOnce(&Consumer) -> Result<T, KafkaError> + Send + 'static,
) -> Result<T, ExecError> {
    let consumer = consumer.clone();
    tokio::task::spawn_blocking(move || {
        call(&consumer).map_err(|error| ExecError::new(call_error(&consumer, &error).1))
    })
    .await
    .map_err(|_| ExecError::new(text::KAFKA_CALL_FAILED))?
}
