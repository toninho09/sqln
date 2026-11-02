//! Safe writes, hashes, and file locks.

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::fs::{self, File, TryLockError};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A temporary file older than this is the remainder of a process that stopped.
pub const OLD_TEMPORARY: Duration = Duration::from_secs(60);

/// Returns the hash of the content of a file. It is valid inside one process.
pub fn hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Reads a UTF-8 file. It returns the text and the hash of the bytes.
pub fn read(path: &Path) -> io::Result<(String, u64)> {
    let bytes = fs::read(path)?;
    let hash = hash(&bytes);
    let text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "The file is not valid UTF-8."))?;
    Ok((text, hash))
}

/// What a write expects to find in the destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// The destination must not exist.
    Missing,
    /// The destination must hold the content with this hash.
    Hash(u64),
    /// Replace the destination, whatever it holds.
    Any,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// The destination holds other content than the write expected.
    Changed,
    /// The destination does not exist, and the write expected a file.
    Removed,
    Failed(String),
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Changed => formatter.write_str("Another program changed the file."),
            WriteError::Removed => formatter.write_str("Another program removed the file."),
            WriteError::Failed(message) => formatter.write_str(message),
        }
    }
}

/// Returns the temporary file of a path, in the same folder.
pub fn temporary(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(ToOwned::to_owned).unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Returns a unique temporary file of a path, in the same folder. Two writers of
/// the same destination never share a temporary file.
pub fn temporary_unique(path: &Path) -> PathBuf {
    path.with_file_name(format!(".{}.tmp", uuid::Uuid::new_v4()))
}

fn check(path: &Path, expect: Expect) -> Result<(), WriteError> {
    match expect {
        Expect::Any => Ok(()),
        Expect::Missing if path.exists() => Err(WriteError::Changed),
        Expect::Missing => Ok(()),
        Expect::Hash(wanted) => match fs::read(path) {
            Ok(bytes) if hash(&bytes) == wanted => Ok(()),
            Ok(_) => Err(WriteError::Changed),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(WriteError::Removed),
            Err(error) => Err(WriteError::Failed(error.to_string())),
        },
    }
}

/// Writes a file in one step: a temporary file in the same folder, a sync, a check of
/// the destination, and a rename. A failure removes the temporary file.
/// It returns the hash of the new content.
pub fn write_atomic(path: &Path, bytes: &[u8], expect: Expect) -> Result<u64, WriteError> {
    write(path, bytes, expect, false)
}

/// Writes with owner-only permissions on Unix, including the temporary file.
/// A unique temporary file prevents reuse of a file or symlink from another writer.
pub fn write_private_atomic(path: &Path, bytes: &[u8], expect: Expect) -> Result<u64, WriteError> {
    write(path, bytes, expect, true)
}

/// Returns the options of a new file that only its owner can read.
fn private_options() -> fs::OpenOptions {
    let mut options = File::options();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

/// Opens a private file to add text at its end. It creates the file and its folders.
pub fn open_private_append(path: &Path) -> io::Result<File> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    private_options().append(true).create(true).open(path)
}

fn write(path: &Path, bytes: &[u8], expect: Expect, private: bool) -> Result<u64, WriteError> {
    let failed = |error: io::Error| WriteError::Failed(error.to_string());
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(failed)?;
    }
    let temporary = if private {
        temporary_unique(path)
    } else {
        temporary(path)
    };
    let mut opened = false;
    let result = (|| {
        let mut file = if private {
            private_options()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(failed)?
        } else {
            File::create(&temporary).map_err(failed)?
        };
        opened = true;
        file.write_all(bytes).map_err(failed)?;
        file.sync_all().map_err(failed)?;
        drop(file);
        check(path, expect)?;
        fs::rename(&temporary, path).map_err(failed)
    })();
    if result.is_err() && opened {
        let _ = fs::remove_file(&temporary);
    }
    result.map(|()| hash(bytes))
}

/// Removes the old temporary files of a folder.
pub fn remove_old_temporary_files(folder: &Path) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "tmp") {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| SystemTime::now().duration_since(time).ok());
        if age.is_some_and(|age| age >= OLD_TEMPORARY) {
            let _ = fs::remove_file(path);
        }
    }
}

/// Returns true when a name comes from `temporary_unique`.
fn is_unique_temporary(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(stem) = name
        .strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    uuid::Uuid::parse_str(stem).is_ok()
}

/// Removes the unique temporary files of exports that no writer holds. A writer
/// locks its file, so a file without a lock comes from a process that stopped.
/// A file younger than `min_age` stays, because its writer can still lock it.
pub fn remove_orphan_temporaries(folder: &Path, min_age: Duration) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_unique_temporary(&path) {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| SystemTime::now().duration_since(time).ok());
        if !age.is_some_and(|age| age >= min_age) {
            continue;
        }
        let Ok(file) = File::options().read(true).write(true).open(&path) else {
            continue;
        };
        if file.try_lock().is_ok() {
            let _ = fs::remove_file(path);
        }
    }
}

/// A lock of the system on a file. The system frees it when the value drops,
/// and when the process ends.
#[derive(Debug)]
pub struct FileLock {
    _file: File,
}

fn open_lock(path: &Path) -> io::Result<File> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

/// Takes the lock at once, or it returns `None` when another process holds it.
pub fn try_lock(path: &Path) -> io::Result<Option<FileLock>> {
    let file = open_lock(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(FileLock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Waits for the lock. A process holds it for a short step only.
pub fn lock(path: &Path) -> io::Result<FileLock> {
    let file = open_lock(path)?;
    file.lock()?;
    Ok(FileLock { _file: file })
}
