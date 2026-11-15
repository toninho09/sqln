//! Machine-local confirmations. Projects never carry trust or password data.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::creds::{Destination, Identity};
use crate::store::file::{self, Expect};
use crate::store::{Scope, Store};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u32,
    #[serde(default)]
    destinations: Vec<Destination>,
    #[serde(default)]
    projects: Vec<KnownProject>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            destinations: Vec::new(),
            projects: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KnownProject {
    connection_id: String,
    paths: Vec<PathBuf>,
}

/// The interface must obtain confirmation for every field that is true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Required {
    pub destination: bool,
    pub project_path: bool,
}

impl Required {
    pub fn any(self) -> bool {
        self.destination || self.project_path
    }
}

/// Error text never includes content from a damaged trust file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustError {
    Read,
    Invalid,
    Write,
    ProjectPath,
}

impl fmt::Display for TrustError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Read => "SQLN cannot read the global trust.json file.",
            Self::Invalid => "The global trust.json file is invalid.",
            Self::Write => "SQLN cannot save the global trust.json file.",
            Self::ProjectPath => "SQLN cannot resolve the project folder.",
        })
    }
}

impl std::error::Error for TrustError {}

/// Always uses the global folder, even when a project is active.
pub struct Trust {
    path: PathBuf,
    lock: PathBuf,
}

impl Trust {
    pub fn new(store: &Store) -> Self {
        Self {
            path: store.global.dir.join("trust.json"),
            lock: store.global.locks_dir().join("trust.lock"),
        }
    }

    /// Reads without creating a file or changing any confirmation.
    pub fn required(
        &self,
        identity: &Identity,
        project: Option<&Path>,
    ) -> Result<Required, TrustError> {
        let project = project_path(identity, project)?;
        let (document, _) = self.read()?;
        let destination = !document.destinations.contains(identity.destination());
        let project_path = project.is_some_and(|path| {
            !document.projects.iter().any(|known| {
                known.connection_id == identity.connection_id() && known.paths.contains(&path)
            })
        });
        Ok(Required {
            destination,
            project_path,
        })
    }

    /// Call only after confirmation, or a local registration authorized by the plan.
    /// A lock and reread preserve confirmations from other SQLN processes.
    pub fn confirm(&self, identity: &Identity, project: Option<&Path>) -> Result<(), TrustError> {
        let project = project_path(identity, project)?;
        let _lock = file::lock(&self.lock).map_err(|_| TrustError::Write)?;
        let (mut document, hash) = self.read()?;
        if !document.destinations.contains(identity.destination()) {
            document.destinations.push(identity.destination().clone());
        }
        if let Some(path) = project {
            match document
                .projects
                .iter_mut()
                .find(|known| known.connection_id == identity.connection_id())
            {
                Some(known) if !known.paths.contains(&path) => known.paths.push(path),
                Some(_) => {}
                None => document.projects.push(KnownProject {
                    connection_id: identity.connection_id().to_owned(),
                    paths: vec![path],
                }),
            }
        }
        let mut bytes = serde_json::to_vec_pretty(&document).map_err(|_| TrustError::Write)?;
        bytes.push(b'\n');
        let expect = hash.map_or(Expect::Missing, Expect::Hash);
        file::write_private_atomic(&self.path, &bytes, expect).map_err(|_| TrustError::Write)?;
        Ok(())
    }

    fn read(&self) -> Result<(Document, Option<u64>), TrustError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((Document::default(), None));
            }
            Err(_) => return Err(TrustError::Read),
        };
        let document: Document = serde_json::from_slice(&bytes).map_err(|_| TrustError::Invalid)?;
        if document.version != 1 {
            return Err(TrustError::Invalid);
        }
        Ok((document, Some(file::hash(&bytes))))
    }
}

fn project_path(identity: &Identity, path: Option<&Path>) -> Result<Option<PathBuf>, TrustError> {
    if identity.scope() == Scope::Global {
        return Ok(None);
    }
    let path = path.ok_or(TrustError::ProjectPath)?;
    let path = fs::canonicalize(path).map_err(|_| TrustError::ProjectPath)?;
    if !path.is_dir() {
        return Err(TrustError::ProjectPath);
    }
    Ok(Some(path))
}
