//! Machine-local consent for SQL and schema transmission.
use crate::store::file::{self, Expect};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub project: Option<PathBuf>,
    pub connection: String,
    pub endpoint: String,
    pub credential: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    destinations: Vec<Destination>,
}
fn load(path: &Path) -> Result<(Document, Option<u64>), String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let d: Document = serde_json::from_slice(&bytes)
                .map_err(|_| "Cannot read ai-trust.json. Check the local consent file.")?;
            if d.version != 1 {
                return Err(
                    "Unsupported ai-trust.json version. Check the local consent file.".into(),
                );
            }
            Ok((d, Some(file::hash(&bytes))))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((
            Document {
                version: 1,
                ..Default::default()
            },
            None,
        )),
        Err(_) => Err("Cannot read ai-trust.json. Check its permissions.".into()),
    }
}
pub fn approved(root: &Path, destination: &Destination) -> Result<bool, String> {
    Ok(load(&root.join("ai-trust.json"))?
        .0
        .destinations
        .contains(destination))
}
pub fn confirm(root: &Path, destination: Destination) -> Result<(), String> {
    let _lock = file::lock(&root.join("locks/ai-trust.lock"))
        .map_err(|_| "Cannot lock AI consent. Try again.")?;
    let path = root.join("ai-trust.json");
    let (mut d, hash) = load(&path)?;
    if !d.destinations.contains(&destination) {
        d.destinations.push(destination);
    }
    let bytes = serde_json::to_vec_pretty(&d).map_err(|_| "Cannot encode AI consent.")?;
    file::write_private_atomic(&path, &bytes, hash.map_or(Expect::Missing, Expect::Hash))
        .map(|_| ())
        .map_err(|_| "Cannot save AI consent. Check write access.".into())
}
