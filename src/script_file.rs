//! The saved state of one console: its files, its lock, and its autosave.
//!
//! The editor owns the text. This value knows what the files hold, and it decides
//! when to write. A failed write never drops the text: the problem stays in
//! `trouble`, and the console keeps working.

use std::io;
use std::time::{Duration, Instant, SystemTime};

use crate::store::file::{Expect, FileLock, WriteError};
use crate::store::scripts::{self, LineEnding, ScriptMeta};
use crate::store::{Root, Scope};

/// The time without an edit before the autosave writes the text.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);
/// The time before the next try after a failed write.
pub const RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trouble {
    /// A write failed. The text stays in memory, and the autosave tries again later.
    Failed(String),
    /// Another program changed the file. The autosave waits for a choice.
    Changed,
    /// Another program removed the file. The autosave waits for a choice.
    Removed,
}

impl Trouble {
    /// Returns true when the autosave waits for a choice of the user.
    pub fn needs_choice(&self) -> bool {
        !matches!(self, Trouble::Failed(_))
    }
}

pub struct ScriptFile {
    pub meta: ScriptMeta,
    pub scope: Scope,
    pub trouble: Option<Trouble>,
    _lock: Option<FileLock>,
    /// The hash of the text file at the last write or read. `None` means no text file.
    hash: Option<u64>,
    meta_written: bool,
    ending: LineEnding,
    /// The revision of the editor that the text file holds.
    saved_revision: u64,
    /// The revision that the autosave saw last, and the time of its next write.
    seen_revision: Option<u64>,
    due_at: Option<Instant>,
    /// The size and the time of the text file at the last check.
    seen: Option<(u64, SystemTime)>,
}

impl ScriptFile {
    /// A console that has no file yet. The first write creates the files.
    pub fn new(scope: Scope, meta: ScriptMeta, lock: Option<FileLock>, revision: u64) -> Self {
        Self {
            meta,
            scope,
            trouble: None,
            _lock: lock,
            hash: None,
            meta_written: false,
            ending: LineEnding::Lf,
            saved_revision: revision,
            seen_revision: None,
            due_at: None,
            seen: None,
        }
    }

    /// A console that shows a file on disk. `hash` is `None` when the text file is absent.
    pub fn open(
        scope: Scope,
        meta: ScriptMeta,
        lock: Option<FileLock>,
        revision: u64,
        loaded: Option<(u64, LineEnding)>,
        root: &Root,
    ) -> Self {
        let mut file = Self::new(scope, meta, lock, revision);
        file.meta_written = true;
        if let Some((hash, ending)) = loaded {
            file.hash = Some(hash);
            file.ending = ending;
            file.seen = scripts::stat(root, &file.meta.id);
        }
        file
    }

    pub fn is_dirty(&self, revision: u64) -> bool {
        self.saved_revision != revision
    }

    /// Returns true when the autosave must write now. The caller passes the clock, so
    /// that each edit restarts the wait, and a test controls the time.
    pub fn due(&mut self, revision: u64, now: Instant) -> bool {
        if self.trouble.as_ref().is_some_and(Trouble::needs_choice) {
            return false;
        }
        if revision == self.saved_revision {
            self.seen_revision = None;
            self.due_at = None;
            return false;
        }
        if self.seen_revision != Some(revision) {
            self.seen_revision = Some(revision);
            self.due_at = Some(now + SAVE_DELAY);
            return false;
        }
        self.due_at.is_some_and(|at| now >= at)
    }

    fn written(&mut self, root: &Root, hash: u64, revision: u64) {
        self.hash = Some(hash);
        self.meta_written = true;
        self.saved_revision = revision;
        self.seen_revision = None;
        self.due_at = None;
        self.trouble = None;
        self.seen = scripts::stat(root, &self.meta.id);
    }

    fn failed(&mut self, error: WriteError, now: Instant) -> Trouble {
        let trouble = match error {
            WriteError::Changed => Trouble::Changed,
            WriteError::Removed => Trouble::Removed,
            WriteError::Failed(message) => {
                self.due_at = Some(now + RETRY_DELAY);
                Trouble::Failed(message)
            }
        };
        self.trouble = Some(trouble.clone());
        trouble
    }

    /// Writes the text. The text has `\n` line ends, and the file keeps its own.
    pub fn save(
        &mut self,
        root: &Root,
        text: &str,
        revision: u64,
        now: Instant,
    ) -> Result<(), Trouble> {
        let bytes = scripts::encode(text, self.ending);
        let result = match (self.hash, self.meta_written) {
            (None, false) => scripts::create(root, &self.meta, &bytes)
                .map_err(WriteError::Failed),
            (None, true) => scripts::write_text(root, &self.meta.id, &bytes, Expect::Missing),
            (Some(hash), _) => scripts::write_text(root, &self.meta.id, &bytes, Expect::Hash(hash)),
        };
        match result {
            Ok(hash) => {
                self.written(root, hash, revision);
                Ok(())
            }
            Err(error) => Err(self.failed(error, now)),
        }
    }

    /// Writes the text over the file, whatever the file holds. The user chose it.
    pub fn overwrite(&mut self, root: &Root, text: &str, revision: u64) -> Result<(), Trouble> {
        let now = Instant::now();
        if scripts::write_meta(root, &self.meta, Expect::Any).is_err() {
            return Err(self.failed(WriteError::Failed("The files cannot be written.".into()), now));
        }
        let bytes = scripts::encode(text, self.ending);
        match scripts::write_text(root, &self.meta.id, &bytes, Expect::Any) {
            Ok(hash) => {
                self.written(root, hash, revision);
                Ok(())
            }
            Err(error) => Err(self.failed(error, now)),
        }
    }

    /// Reads the file again. The caller replaces the editor, and it passes the
    /// revision of the new editor to `adopt`.
    pub fn reload(&mut self, root: &Root) -> io::Result<String> {
        let (text, hash) = scripts::read_text(root, &self.meta.id)?;
        let (text, ending) = scripts::decode(&text);
        self.hash = Some(hash);
        self.ending = ending;
        self.seen = scripts::stat(root, &self.meta.id);
        self.trouble = None;
        self.seen_revision = None;
        self.due_at = None;
        Ok(text)
    }

    /// Records that the editor holds the text of the file at this revision.
    pub fn adopt(&mut self, revision: u64) {
        self.saved_revision = revision;
    }

    /// Follows a new editor that holds the same text. `dirty` says whether the text
    /// of the new editor is newer than the file.
    pub fn rebase(&mut self, revision: u64, dirty: bool) {
        self.saved_revision = if dirty { revision.wrapping_add(1) } else { revision };
        self.seen_revision = None;
        self.due_at = None;
    }

    /// The user closes the console without the pending text.
    pub fn discard(&mut self, revision: u64) {
        self.saved_revision = revision;
        self.trouble = None;
    }

    /// Writes the identity file again, after a change of the schema.
    pub fn write_meta(&mut self, root: &Root) -> Result<(), String> {
        if !self.meta_written {
            return Ok(());
        }
        scripts::write_meta(root, &self.meta, Expect::Any)
    }

    /// Replaces the place of the script after a move to another scope.
    pub fn relocate(&mut self, scope: Scope, meta: ScriptMeta, lock: Option<FileLock>, root: &Root) {
        self.scope = scope;
        self.meta = meta;
        self._lock = lock;
        self.seen = scripts::stat(root, &self.meta.id);
        // A move copies the saved text, so the hash of the new file is the one read.
        self.hash = scripts::read_text(root, &self.meta.id).ok().map(|(_, hash)| hash);
        self.meta_written = true;
    }

    /// Looks for a change of the files by another program. It reads the file only when
    /// its size or its time changed.
    pub fn poll(&mut self, root: &Root) {
        if self.trouble.as_ref().is_some_and(Trouble::needs_choice) {
            return;
        }
        let current = scripts::stat(root, &self.meta.id);
        match (self.hash, current) {
            (None, Some(_)) => self.trouble = Some(Trouble::Changed),
            (Some(_), None) => self.trouble = Some(Trouble::Removed),
            (Some(hash), Some(now)) if self.seen != Some(now) => {
                match scripts::read_text(root, &self.meta.id) {
                    Ok((_, found)) if found == hash => self.seen = Some(now),
                    _ => self.trouble = Some(Trouble::Changed),
                }
            }
            _ => {}
        }
    }
}
