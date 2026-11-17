//! Bounded HTTP calls. External response bodies never enter error messages.
use super::{Prepared, Provider};
use crate::connector::target::Secret;
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const RESPONSE_LIMIT: usize = 1024 * 1024;

fn client(provider: &Provider) -> Result<reqwest::Client, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(crate::connector::tls::system_roots().iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| "Cannot configure API TLS. Check the system TLS configuration.")?
    .with_root_certificates(roots)
    .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(provider.timeout_seconds))
        .build()
        .map_err(|_| "Cannot create the API client. Check the system TLS configuration.".into())
}
fn request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "The AI time limit ended. Try again or increase the timeout."
    } else {
        "Cannot contact the AI endpoint. Check the endpoint, network, and TLS certificates."
    }
    .into()
}
async fn receive(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 | 403 => "The API refuses this key. Check the credential and model access.",
            402 => {
                "The API balance is insufficient (HTTP 402). Check the provider account balance."
            }
            404 => "The API route is not found (HTTP 404). Check the endpoint and protocol.",
            429 => "The API limit is reached. Wait before trying again.",
            300..=399 => "The API redirects this request. Configure the final endpoint explicitly.",
            _ => "The API refuses this request. Check the model, endpoint, and context limits.",
        }
        .into());
    }
    if response
        .content_length()
        .is_some_and(|n| n > RESPONSE_LIMIT as u64)
    {
        return Err("The API response exceeds 1 MiB. Use a smaller output limit.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        if bytes.len() + chunk.len() > RESPONSE_LIMIT {
            return Err("The API response exceeds 1 MiB. Use a smaller output limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Sends a bounded chat request. Both completion and explanation use this transport.
async fn answer(
    provider: &Provider,
    prepared: &Prepared,
    key: Option<&Secret>,
) -> Result<(String, Option<String>), String> {
    let mut request = client(provider)?
        .post(provider.completion_url()?)
        .json(&prepared.body);
    if let Some(key) = key {
        request = request.bearer_auth(key.expose());
    }
    let bytes = receive(request.send().await.map_err(request_error)?).await?;
    let json: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "The API returns invalid JSON. Check its protocol.")?;
    let choice = json
        .get("choices")
        .and_then(|choices| choices.get(0))
        .ok_or("The API returns no choice. Check the model and protocol.")?;
    let text = choice
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .ok_or("The API returns no answer text. Choose a chat model.")?;
    let reason = choice.get("finish_reason").and_then(Value::as_str);
    Ok((text.to_owned(), reason.map(str::to_owned)))
}

pub async fn complete(
    provider: &Provider,
    prepared: &Prepared,
    key: Option<&Secret>,
    original: &str,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let operation = async {
        let (text, reason) = answer(provider, prepared, key).await?;
        super::replacement(&text, reason.as_deref(), original)
    };
    tokio::select! { result = operation => result, () = cancel.cancelled() => Err("AI completion cancelled.".into()) }
}

pub async fn test(
    provider: &Provider,
    key: Option<&Secret>,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let operation = async {
        let mut request = client(provider)?.get(provider.models_url()?);
        if let Some(key) = key {
            request = request.bearer_auth(key.expose());
        }
        let _ = receive(request.send().await.map_err(request_error)?).await?;
        Ok(())
    };
    tokio::select! { result = operation => result, () = cancel.cancelled() => Err("AI connection test cancelled.".into()) }
}

/// Sends one chat question. The caller shows the answer in a window, never in the editor.
pub async fn ask(
    provider: &Provider,
    prepared: &Prepared,
    key: Option<&Secret>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let operation = async {
        let (text, reason) = answer(provider, prepared, key).await?;
        if reason.as_deref() != Some("stop") {
            return Err(
                "The AI answer is incomplete. Increase Output tokens or try another model.".into(),
            );
        }
        if text.trim().is_empty() {
            return Err("The API returns an empty answer. Choose a chat model.".into());
        }
        Ok(text)
    };
    tokio::select! { result = operation => result, () = cancel.cancelled() => Err("AI explanation cancelled.".into()) }
}
