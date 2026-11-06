//! API credentials. Read rotating files on each use, never during drawing.
use super::{Credential, Provider};
use crate::connector::target::Secret;
use crate::creds::SystemVault;
use std::io::Read;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

pub fn account(scope: &str, id: &str, endpoint: &str) -> String {
    serde_json::json!([scope, id, endpoint]).to_string()
}
pub fn file_path(base: &Path, path: &str) -> PathBuf {
    base.join(path)
}

pub fn read_file(path: &Path) -> Result<Secret, String> {
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return Err("The key path must refer to a readable regular file.".into());
    }
    let file = std::fs::File::open(path)
        .map_err(|_| "Cannot open the key file. Check its path and access.")?;
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Err("The key path must refer to a regular file.".into());
    }
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read the key file. Check access and try again.")?;
    if bytes.len() > 65536 {
        bytes.zeroize();
        return Err("The key file exceeds 64 KiB. Supply a file containing only the key.".into());
    }
    let value = std::str::from_utf8(&bytes)
        .ok()
        .map(|s| s.trim_end_matches(['\r', '\n']));
    let key = value
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .map(|s| Secret::new(s.to_owned()));
    bytes.zeroize();
    key.ok_or_else(|| {
        "The key file must contain a UTF-8 key without internal control characters.".into()
    })
}
pub fn read(
    provider: &Provider,
    scope: &str,
    base: &Path,
    vault: &SystemVault,
) -> Result<Option<Secret>, String> {
    match &provider.credential {
        Credential::None => Ok(None),
        Credential::File { path } => read_file(&file_path(base, path)).map(Some),
        Credential::Vault { id, endpoint } => {
            if provider.endpoint()? != *endpoint {
                return Err("This key belongs to another endpoint. Configure a new key.".into());
            }
            vault
                .read_api_key(&account(scope, id, endpoint))
                .map_err(|e| format!("{e} Retry, use a session key, or select a key file."))?
                .map(Some)
                .ok_or_else(|| {
                    "The vault has no API key. Save a key in Settings or select a key file.".into()
                })
        }
    }
}
