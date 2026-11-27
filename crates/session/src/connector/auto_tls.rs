//! The automatic TLS mode. When the user made no TLS choice, a connector tries the
//! other mode once, but only after the server refused the first mode. A certificate
//! error never causes a connection without TLS.

use std::future::Future;
use std::sync::Mutex;

use crate::text;

use super::target::{NetworkTarget, Tls, TlsFiles};
use super::{ConnectError, TlsRefusal};

/// Keeps the mode that connected, so that later sessions of the pool use it directly.
#[derive(Debug, Default)]
pub struct AutoTls {
    chosen: Mutex<Option<Tls>>,
}

impl AutoTls {
    /// Runs `open` with a target that has a resolved TLS mode. A target with an explicit
    /// mode gets one attempt. Returns the notice of a fallback to a connection without
    /// TLS.
    pub async fn connect<S, F, Fut>(
        &self,
        target: &NetworkTarget,
        open: F,
    ) -> Result<(S, Option<&'static str>), ConnectError>
    where
        F: Fn(NetworkTarget) -> Fut,
        Fut: Future<Output = Result<S, ConnectError>>,
    {
        if target.tls != Tls::Auto {
            return Ok((open(target.clone()).await?, None));
        }
        if let Some(tls) = self.chosen() {
            return Ok((open(with_tls(target, tls)).await?, None));
        }
        let first = target.tls.first_try(&target.address);
        let error = match open(with_tls(target, first.clone())).await {
            Ok(session) => {
                self.remember(first);
                return Ok((session, None));
            }
            Err(error) => error,
        };
        let (second, notice) = match (&first, error.tls_refused) {
            (Tls::Verify(_), Some(TlsRefusal::ServerHasNoTls)) => {
                (Tls::Disabled, Some(text::TLS_FALLBACK_NOTICE))
            }
            (Tls::Disabled, Some(TlsRefusal::ServerNeedsTls)) => {
                (Tls::Verify(TlsFiles::default()), None)
            }
            _ => return Err(error),
        };
        match open(with_tls(target, second.clone())).await {
            Ok(session) => {
                self.remember(second);
                Ok((session, notice))
            }
            Err(retry) => Err(both_failed(&first, &error, retry)),
        }
    }

    fn chosen(&self) -> Option<Tls> {
        self.chosen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn remember(&self, tls: Tls) {
        *self
            .chosen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tls);
    }
}

fn with_tls(target: &NetworkTarget, tls: Tls) -> NetworkTarget {
    NetworkTarget {
        tls,
        ..target.clone()
    }
}

/// Keeps the facts of the retry error and adds the message of the first attempt.
fn both_failed(first: &Tls, error: &ConnectError, mut retry: ConnectError) -> ConnectError {
    let (before, after) = match first {
        Tls::Disabled => ("Without TLS", "With TLS"),
        _ => ("With TLS", "Without TLS"),
    };
    retry.message = format!("{before}: {}. {after}: {}", error.message, retry.message);
    retry.tls_refused = None;
    retry
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::connector::target::{Address, DatabricksOptions, Engine, KafkaOptions};

    fn target(host: &str, tls: Tls) -> NetworkTarget {
        NetworkTarget {
            engine: Engine::Postgres,
            address: Address::Tcp {
                host: host.to_owned(),
                port: 5432,
            },
            user: "u".to_owned(),
            password: None,
            database: None,
            tls,
            kafka: KafkaOptions::default(),
            databricks: DatabricksOptions::default(),
        }
    }

    /// Answers each attempt from its TLS mode and records the modes.
    fn run(
        auto: &AutoTls,
        target: &NetworkTarget,
        answer: impl Fn(&Tls) -> Result<(), ConnectError>,
    ) -> (Result<Option<&'static str>, ConnectError>, Vec<Tls>) {
        let attempts = Mutex::new(Vec::new());
        let calls = AtomicUsize::new(0);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        let result = runtime.block_on(auto.connect(target, |resolved| {
            calls.fetch_add(1, Ordering::SeqCst);
            attempts.lock().unwrap().push(resolved.tls.clone());
            let result = answer(&resolved.tls);
            async move { result }
        }));
        let attempts = attempts.into_inner().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), attempts.len());
        (result.map(|((), notice)| notice), attempts)
    }

    fn verify() -> Tls {
        Tls::Verify(TlsFiles::default())
    }

    fn refused(refusal: TlsRefusal) -> ConnectError {
        ConnectError::new("refused").tls_refused(refusal)
    }

    #[test]
    fn a_remote_server_without_tls_connects_without_tls_with_a_notice() {
        let auto = AutoTls::default();
        let remote = target("db.example.test", Tls::Auto);
        let (result, attempts) = run(&auto, &remote, |tls| match tls {
            Tls::Verify(_) => Err(refused(TlsRefusal::ServerHasNoTls)),
            _ => Ok(()),
        });
        assert_eq!(result.unwrap(), Some(text::TLS_FALLBACK_NOTICE));
        assert_eq!(attempts, [verify(), Tls::Disabled]);

        // The next session of the pool goes directly without TLS and without a notice.
        let (result, attempts) = run(&auto, &remote, |_| Ok(()));
        assert_eq!(result.unwrap(), None);
        assert_eq!(attempts, [Tls::Disabled]);
    }

    #[test]
    fn a_certificate_error_does_not_try_without_tls() {
        let auto = AutoTls::default();
        let remote = target("db.example.test", Tls::Auto);
        let (result, attempts) = run(&auto, &remote, |_| Err(ConnectError::new("bad cert")));
        assert_eq!(result.unwrap_err().message, "bad cert");
        assert_eq!(attempts, [verify()]);
    }

    #[test]
    fn a_local_server_that_needs_tls_connects_with_tls_without_a_notice() {
        let auto = AutoTls::default();
        let local = target("localhost", Tls::Auto);
        let (result, attempts) = run(&auto, &local, |tls| match tls {
            Tls::Disabled => Err(refused(TlsRefusal::ServerNeedsTls)),
            _ => Ok(()),
        });
        assert_eq!(result.unwrap(), None);
        assert_eq!(attempts, [Tls::Disabled, verify()]);
    }

    #[test]
    fn a_refusal_of_the_other_direction_does_not_retry() {
        let auto = AutoTls::default();
        let local = target("127.0.0.1", Tls::Auto);
        let (result, attempts) = run(&auto, &local, |_| Err(refused(TlsRefusal::ServerHasNoTls)));
        assert!(result.is_err());
        assert_eq!(attempts, [Tls::Disabled]);
    }

    #[test]
    fn an_explicit_mode_gets_one_attempt() {
        for tls in [verify(), Tls::Disabled] {
            let auto = AutoTls::default();
            let explicit = target("db.example.test", tls.clone());
            let (result, attempts) = run(&auto, &explicit, |_| {
                Err(refused(TlsRefusal::ServerHasNoTls))
            });
            assert_eq!(
                result.unwrap_err().tls_refused,
                Some(TlsRefusal::ServerHasNoTls)
            );
            assert_eq!(attempts, [tls]);
        }
    }

    #[test]
    fn a_failed_retry_keeps_both_messages() {
        let auto = AutoTls::default();
        let local = target("localhost", Tls::Auto);
        let (result, attempts) = run(&auto, &local, |tls| match tls {
            Tls::Disabled => {
                Err(ConnectError::new("no encryption").tls_refused(TlsRefusal::ServerNeedsTls))
            }
            _ => Err(ConnectError::new("unknown CA")),
        });
        let error = result.unwrap_err();
        assert_eq!(
            error.message,
            "Without TLS: no encryption. With TLS: unknown CA"
        );
        assert_eq!(error.tls_refused, None);
        assert_eq!(attempts.len(), 2);
    }
}
