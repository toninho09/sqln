//! The saved state of one console: its files, its lock, and its autosave.
//!
//! The editor owns the text. This value knows what the files hold, and it decides
//! when to write. A failed write never drops the text: the problem stays in
//! `trouble`, and the console keeps working.

mod worker;

use worker::PendingSave;
pub(crate) use worker::SaveWorker;

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

struct Saved {
    hash: u64,
    seen: Option<(u64, SystemTime)>,
}

type SaveReply = (Option<FileLock>, Result<SaveUpdate, WriteError>);

struct SaveUpdate {
    text: Option<Saved>,
    relocated: Option<Relocation>,
}

struct Relocation {
    scope: Scope,
    meta: ScriptMeta,
    lock: Option<FileLock>,
    hash: Option<u64>,
    seen: Option<(u64, SystemTime)>,
}

struct SaveRequest {
    root: Root,
    meta: ScriptMeta,
    text: String,
    ending: LineEnding,
    mode: SaveMode,
    acquire_lock: bool,
}

enum SaveMode {
    Create,
    Check(Expect),
    Overwrite,
    Metadata,
    Relocate {
        from: Root,
        scope: Scope,
        hash: Option<u64>,
        moved: bool,
    },
}

impl SaveRequest {
    fn run(self) -> SaveReply {
        let lock = if self.acquire_lock {
            scripts::lock(&self.root, &self.meta.id).ok().flatten()
        } else {
            None
        };
        (lock, self.write())
    }

    fn write(self) -> Result<SaveUpdate, WriteError> {
        match self.mode {
            SaveMode::Metadata => {
                scripts::write_meta(&self.root, &self.meta, Expect::Any)
                    .map_err(WriteError::Failed)?;
                return Ok(SaveUpdate {
                    text: None,
                    relocated: None,
                });
            }
            SaveMode::Relocate {
                ref from,
                scope,
                hash,
                moved,
            } => {
                let (hash, lock) = if moved {
                    let lock = scripts::lock(&self.root, &self.meta.id)
                        .map_err(|error| WriteError::Failed(error.to_string()))?
                        .ok_or_else(|| {
                            WriteError::Failed(crate::ui::text::SCRIPT_BUSY.to_owned())
                        })?;
                    let hash = scripts::move_to(from, &self.root, &self.meta)
                        .map_err(WriteError::Failed)?;
                    (Some(hash), Some(lock))
                } else {
                    scripts::write_meta(&self.root, &self.meta, Expect::Any)
                        .map_err(WriteError::Failed)?;
                    (hash, None)
                };
                let seen = scripts::stat(&self.root, &self.meta.id);
                return Ok(SaveUpdate {
                    text: None,
                    relocated: Some(Relocation {
                        scope,
                        meta: self.meta,
                        lock,
                        hash,
                        seen,
                    }),
                });
            }
            _ => {}
        }
        let bytes = scripts::encode(&self.text, self.ending);
        let hash = match self.mode {
            SaveMode::Create => {
                scripts::create(&self.root, &self.meta, &bytes).map_err(WriteError::Failed)?
            }
            SaveMode::Check(expect) => {
                scripts::write_text(&self.root, &self.meta.id, &bytes, expect)?
            }
            SaveMode::Overwrite => {
                scripts::write_meta(&self.root, &self.meta, Expect::Any)
                    .map_err(WriteError::Failed)?;
                scripts::write_text(&self.root, &self.meta.id, &bytes, Expect::Any)?
            }
            SaveMode::Metadata | SaveMode::Relocate { .. } => unreachable!(),
        };
        Ok(SaveUpdate {
            text: Some(Saved {
                hash,
                seen: scripts::stat(&self.root, &self.meta.id),
            }),
            relocated: None,
        })
    }
}

pub struct ScriptFile {
    pub meta: ScriptMeta,
    pub scope: Scope,
    pub trouble: Option<Trouble>,
    pending: Option<PendingSave>,
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
            pending: None,
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

    fn written(&mut self, saved: Saved, revision: u64) {
        self.hash = Some(saved.hash);
        self.meta_written = true;
        self.saved_revision = revision;
        if self.seen_revision.is_none_or(|seen| seen == revision) {
            self.seen_revision = None;
            self.due_at = None;
        }
        self.trouble = None;
        self.seen = saved.seen;
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
        self.wait_save(now)?;
        let request = self.request(root, text.to_owned(), false);
        self.apply_save(request.run(), revision, now)
    }

    /// Writes the text over the file, whatever the file holds. The user chose it.
    pub fn overwrite(&mut self, root: &Root, text: &str, revision: u64) -> Result<(), Trouble> {
        let now = Instant::now();
        let _ = self.wait_save(now);
        let request = self.request(root, text.to_owned(), true);
        self.apply_save(request.run(), revision, now)
    }

    fn request(&self, root: &Root, text: String, overwrite: bool) -> SaveRequest {
        let mode = if overwrite {
            SaveMode::Overwrite
        } else {
            match (self.hash, self.meta_written) {
                (None, false) => SaveMode::Create,
                (None, true) => SaveMode::Check(Expect::Missing),
                (Some(hash), _) => SaveMode::Check(Expect::Hash(hash)),
            }
        };
        SaveRequest {
            root: root.clone(),
            meta: self.meta.clone(),
            text,
            ending: self.ending,
            mode,
            acquire_lock: !self.meta_written && self._lock.is_none(),
        }
    }

    pub(crate) fn meta_written(&self) -> bool {
        self.meta_written
    }

    pub fn is_saving(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn start_save(
        &mut self,
        worker: &SaveWorker,
        root: &Root,
        text: String,
        revision: u64,
        overwrite: bool,
    ) {
        self.start_request(worker, self.request(root, text, overwrite), revision);
    }

    fn apply_save(&mut self, reply: SaveReply, revision: u64, now: Instant) -> Result<(), Trouble> {
        let (lock, result) = reply;
        if let Some(lock) = lock {
            self._lock = Some(lock);
        }
        match result {
            Ok(update) => {
                if let Some(saved) = update.text {
                    self.written(saved, revision);
                }
                if let Some(relocated) = update.relocated {
                    if self.scope != relocated.scope {
                        self._lock = relocated.lock;
                    }
                    self.scope = relocated.scope;
                    self.meta = relocated.meta;
                    self.hash = relocated.hash;
                    self.seen = relocated.seen;
                    self.meta_written = true;
                }
                Ok(())
            }
            Err(error) => Err(self.failed(error, now)),
        }
    }

    pub(crate) fn start_metadata(&mut self, worker: &SaveWorker, root: &Root) {
        let mut request = self.request(root, String::new(), false);
        request.mode = SaveMode::Metadata;
        self.start_request(worker, request, self.saved_revision);
    }

    pub(crate) fn start_relocate(
        &mut self,
        worker: &SaveWorker,
        from: &Root,
        to: &Root,
        scope: Scope,
        meta: ScriptMeta,
    ) {
        let request = SaveRequest {
            root: to.clone(),
            meta,
            text: String::new(),
            ending: self.ending,
            acquire_lock: false,
            mode: SaveMode::Relocate {
                from: from.clone(),
                scope,
                hash: self.hash,
                moved: scope != self.scope,
            },
        };
        self.start_request(worker, request, self.saved_revision);
    }

    fn start_request(&mut self, worker: &SaveWorker, request: SaveRequest, revision: u64) {
        assert!(!self.is_saving() && worker.ready());
        let metadata_name =
            (!matches!(&request.mode, SaveMode::Check(_))).then(|| request.meta.name.clone());
        self.pending = Some(PendingSave {
            revision,
            metadata_name,
            receiver: Some(worker.submit(request)),
        });
    }

    /// Applies a finished write without waiting for the worker.
    pub(crate) fn poll_save(
        &mut self,
        now: Instant,
    ) -> Option<(Result<(), Trouble>, Option<String>)> {
        let pending = self.pending.as_ref()?;
        let result = match pending.receiver.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => (
                None,
                Err(WriteError::Failed("The save worker stopped.".into())),
            ),
        };
        let mut pending = self.pending.take().unwrap();
        pending.receiver.take();
        Some((
            self.apply_save(result, pending.revision, now),
            pending.metadata_name.take(),
        ))
    }

    pub(crate) fn wait_save(&mut self, now: Instant) -> Result<(), Trouble> {
        let Some(mut pending) = self.pending.take() else {
            return Ok(());
        };
        let result = pending.receiver.take().unwrap().recv().unwrap_or_else(|_| {
            (
                None,
                Err(WriteError::Failed("The save worker stopped.".into())),
            )
        });
        self.apply_save(result, pending.revision, now)
    }

    /// Reads the file again. The caller replaces the editor, and it passes the
    /// revision of the new editor to `adopt`.
    pub fn reload(&mut self, root: &Root) -> io::Result<String> {
        let _ = self.wait_save(Instant::now());
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
        self.saved_revision = if dirty {
            revision.wrapping_add(1)
        } else {
            revision
        };
        self.seen_revision = None;
        self.due_at = None;
    }

    /// The user closes the console without the pending text.
    pub fn discard(&mut self, revision: u64) {
        let _ = self.wait_save(Instant::now());
        self.saved_revision = revision;
        self.trouble = None;
    }

    /// Writes the identity file again, after a change of the schema.
    pub fn write_meta(&mut self, root: &Root) -> Result<(), String> {
        self.wait_save(Instant::now())
            .map_err(|error| format!("{error:?}"))?;
        if !self.meta_written {
            return Ok(());
        }
        scripts::write_meta(root, &self.meta, Expect::Any)
    }

    /// Replaces the place of the script after a move to another scope.
    pub fn relocate(
        &mut self,
        scope: Scope,
        meta: ScriptMeta,
        lock: Option<FileLock>,
        root: &Root,
    ) {
        self.scope = scope;
        self.meta = meta;
        self._lock = lock;
        self.seen = scripts::stat(root, &self.meta.id);
        // A move copies the saved text, so the hash of the new file is the one read.
        self.hash = scripts::read_text(root, &self.meta.id)
            .ok()
            .map(|(_, hash)| hash);
        self.meta_written = true;
    }

    /// Looks for a change of the files by another program. It reads the file only when
    /// its size or its time changed.
    pub fn poll(&mut self, root: &Root) {
        if self.is_saving() {
            return;
        }
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
