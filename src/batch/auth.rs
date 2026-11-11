//! Credential and trust checks before batch execution.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};

use zeroize::Zeroizing;

use super::{Error, Options};
use crate::connection::{ConnectionKey, Definition, Source};
use crate::connector::target::Secret;
use crate::creds::{Credentials, Identity, Lookup, PlainFile, SystemVault};
use crate::grid::printable;
use crate::store::{Scope, config};
use crate::trust::Trust;

/// Uses the controlling terminal, never the SQL input or the data output.
pub(super) fn confirm(question: &str, next: &str) -> Result<(), Error> {
    #[cfg(unix)]
    {
        let mut terminal = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|_| Error::failed(format!("A terminal confirmation is required. {next}")))?;
        write!(terminal, "sqln: {} [y/N] ", printable(question))?;
        terminal.flush()?;
        let mut answer = String::new();
        BufReader::new(terminal).read_line(&mut answer)?;
        if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Ok(());
        }
        Err(Error::failed("The operation is not authorized."))
    }
    #[cfg(not(unix))]
    {
        let _ = question;
        Err(Error::failed(format!(
            "Terminal confirmation is not available. {next}"
        )))
    }
}

pub(super) fn authenticate(
    definition: &mut Definition,
    stored: Option<&config::Stored>,
    options: &Options,
) -> Result<(), Error> {
    if !matches!(definition.source, Source::Network(_)) {
        return Ok(());
    }
    let identity = stored.and_then(|stored| {
        Identity::of(
            &ConnectionKey {
                scope: stored.scope,
                id: stored.id.clone(),
            },
            definition,
        )
    });
    if let Some(identity) = &identity {
        let project = options
            .store
            .project
            .as_ref()
            .filter(|_| identity.scope() == Scope::Project)
            .map(|root| root.base.as_path());
        let trust = Trust::new(&options.store);
        if trust
            .required(identity, project)
            .map_err(|error| Error::failed(error.to_string()))?
            .any()
        {
            confirm(
                &format!(
                    "Trust connection {} at {}? Project: {}",
                    definition.name,
                    definition.summary(),
                    project.map_or_else(|| "global".to_owned(), |path| path.display().to_string())
                ),
                "Open this saved connection interactively to confirm its destination and project. --yes does not authorize trust.",
            )?;
            trust
                .confirm(identity, project)
                .map_err(|error| Error::failed(error.to_string()))?;
        }
    }
    let Source::Network(target) = &mut definition.source else {
        unreachable!()
    };
    let explicit = if let Some(path) = &options.password_file {
        let mut text = Zeroizing::new(std::fs::read_to_string(path)
            .map_err(|_| Error::failed("SQLN cannot read the password file. Check its path, permissions, and UTF-8 encoding."))?);
        if text.ends_with('\n') {
            text.pop();
            if text.ends_with('\r') {
                text.pop();
            }
        }
        Some(Secret::new(text.as_str()))
    } else {
        match std::env::var("SQLN_PASSWORD") {
            Ok(text) => {
                let text = Zeroizing::new(text);
                Some(Secret::new(text.as_str()))
            }
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err(Error::failed("SQLN_PASSWORD must contain valid UTF-8.")),
        }
    };
    if let Some(password) = explicit {
        target.password = Some(password);
        return Ok(());
    }
    if target.password.is_some() {
        return Ok(());
    }
    let reference = stored.is_some_and(|stored| stored.credential != config::CredentialName::None);
    if reference && let Some(identity) = &identity {
        let mut credentials = Credentials::new(SystemVault::new())
            .with_plain(Some(PlainFile::new(&options.store.global)));
        match credentials.lookup(
            identity,
            stored.is_some_and(|stored| stored.credential == config::CredentialName::Vault),
        ) {
            Ok(Lookup::Found(password) | Lookup::Migrate(password)) => {
                target.password = Some(password);
                return Ok(());
            }
            Ok(Lookup::Missing | Lookup::NoVault) => {}
            Err(error) => {
                return Err(Error::failed(format!(
                    "{error} Use --password-file or SQLN_PASSWORD."
                )));
            }
        }
    }
    // Password-free authentication remains available, including local PostgreSQL sessions.
    #[cfg(unix)]
    let terminal = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok();
    #[cfg(not(unix))]
    let terminal: Option<std::fs::File> = None;
    // A Kafka user always authenticates with SASL, so it always needs a password.
    let sasl = target.engine == crate::connector::target::Engine::Kafka && !target.user.is_empty();
    if let Some(mut terminal) = terminal.filter(|_| target.engine.needs_user() || reference || sasl)
    {
        target.password = crate::terminal::read_password_to(
            &format!("Password for {}: ", printable(&target.user)),
            &mut terminal,
        )?;
    } else if reference {
        return Err(Error::failed(
            "The saved credential is not available. Set --password-file or SQLN_PASSWORD.",
        ));
    }
    Ok(())
}
