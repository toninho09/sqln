//! The link of a console to an external `.sql` file. The editor owns the text, and
//! an explicit save writes the file. There is no autosave and no background watch:
//! a write of a file of the user needs an action, and a change by another program
//! appears when the user asks to write.

use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::Trouble;
use crate::store::file::{self, Expect, WriteError};
use crate::store::scripts::{self, LineEnding};

const BOM: &[u8] = "\u{feff}".as_bytes();

/// A file that an open read: its link and its text.
#[derive(Debug)]
pub struct ReadExternal {
    pub file: ExternalFile,
    pub text: String,
}

#[derive(Debug)]
pub struct ExternalFile {
    pub path: PathBuf,
    /// The resolved destination, so a save does not replace a symbolic link.
    target: PathBuf,
    symlink: bool,
    permissions: fs::Permissions,
    pub trouble: Option<Trouble>,
    /// The hash of the file at the last read or write.
    hash: u64,
    /// The revision of the editor that the file holds.
    saved_revision: u64,
    ending: LineEnding,
    /// Original text, used to retain mixed line endings on unchanged lines.
    raw: String,
    /// True when the file starts with a byte order mark, which a write keeps.
    bom: bool,
}

/// Retains the line endings of matching lines and pairs changed lines in order.
/// New lines use the file's existing default. The diff has a bounded runtime.
fn encode_mixed(raw: &str, text: &str, default: LineEnding) -> String {
    let (old_text, _) = scripts::decode(raw);
    let old: Vec<&str> = old_text.split('\n').collect();
    let new: Vec<&str> = text.split('\n').collect();
    let mut endings: Vec<bool> = raw.split('\n').map(|line| line.ends_with('\r')).collect();
    if let Some(last) = endings.last_mut() {
        // The last segment has no line ending, so an added newline uses the default.
        *last = default == LineEnding::Crlf;
    }
    let mut mapped = vec![default == LineEnding::Crlf; new.len()];
    let ops = similar::capture_diff_slices_deadline(
        similar::Algorithm::Myers,
        &old,
        &new,
        Some(Instant::now() + Duration::from_millis(100)),
    );
    let mut pending: Option<(std::ops::Range<usize>, std::ops::Range<usize>)> = None;
    let apply = |a: std::ops::Range<usize>, b: std::ops::Range<usize>, mapped: &mut [bool]| {
        for (old_index, new_index) in a.zip(b) {
            mapped[new_index] = endings[old_index];
        }
    };
    for op in ops {
        let (tag, a, b) = op.as_tag_tuple();
        if tag == similar::DiffTag::Equal {
            if let Some((a, b)) = pending.take() {
                apply(a, b, &mut mapped);
            }
            apply(a, b, &mut mapped);
        } else if let Some((old_range, new_range)) = &mut pending {
            old_range.end = a.end;
            new_range.end = b.end;
        } else {
            pending = Some((a, b));
        }
    }
    if let Some((a, b)) = pending {
        apply(a, b, &mut mapped);
    }
    let mut result = String::with_capacity(text.len());
    for (index, line) in new.iter().enumerate() {
        result.push_str(line);
        if index + 1 < new.len() {
            if mapped[index] {
                result.push('\r');
            }
            result.push('\n');
        }
    }
    result
}

impl ExternalFile {
    /// Reads a file that exists. A file that is not UTF-8 is an error, because the
    /// editor holds text only.
    pub fn read(path: &Path) -> io::Result<ReadExternal> {
        let symlink = fs::symlink_metadata(path)?.file_type().is_symlink();
        let target = fs::canonicalize(path)?;
        let permissions = fs::metadata(&target)?.permissions();
        let bytes = fs::read(&target)?;
        let bom = bytes.starts_with(BOM);
        let body = if bom { &bytes[BOM.len()..] } else { &bytes[..] };
        let raw = String::from_utf8(body.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "The file is not UTF-8."))?;
        let hash = file::hash(&bytes);
        let (text, ending) = scripts::decode(&raw);
        Ok(ReadExternal {
            file: Self {
                path: path.to_owned(),
                target,
                symlink,
                permissions,
                trouble: None,
                hash,
                saved_revision: 0,
                ending,
                raw,
                bom,
            },
            text,
        })
    }

    pub fn is_dirty(&self, revision: u64) -> bool {
        self.saved_revision != revision
    }

    fn bytes<'a>(&self, text: &'a str) -> Cow<'a, [u8]> {
        let mixed = self
            .raw
            .as_bytes()
            .iter()
            .enumerate()
            .any(|(i, byte)| *byte == b'\n' && (i == 0 || self.raw.as_bytes()[i - 1] != b'\r'))
            && self.raw.contains("\r\n");
        if !mixed && self.ending == LineEnding::Lf && !self.bom {
            return Cow::Borrowed(text.as_bytes());
        }
        let encoded = if mixed {
            Cow::Owned(encode_mixed(&self.raw, text, self.ending))
        } else {
            scripts::encode(text, self.ending)
        };
        let mut bytes = Vec::with_capacity(BOM.len() + encoded.len());
        if self.bom {
            bytes.extend_from_slice(BOM);
        }
        bytes.extend_from_slice(encoded.as_bytes());
        Cow::Owned(bytes)
    }

    fn write(&mut self, text: &str, expect: Expect, revision: u64) -> Result<(), Trouble> {
        let destination = match fs::canonicalize(&self.path) {
            Ok(target) if target == self.target => Ok(()),
            Ok(_) => Err(Trouble::Failed(
                "The file link changed; reopen the file before saving.".into(),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound && !self.symlink => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound && expect != Expect::Any => {
                Err(Trouble::Removed)
            }
            Err(error) => Err(Trouble::Failed(error.to_string())),
        };
        if let Err(trouble) = destination {
            self.trouble = Some(trouble.clone());
            return Err(trouble);
        }
        let permissions = fs::metadata(&self.target)
            .map(|metadata| metadata.permissions())
            .unwrap_or_else(|_| self.permissions.clone());
        let bytes = self.bytes(text).into_owned();
        let trouble = match file::write_external_atomic(&self.target, &bytes, expect, &permissions)
        {
            Ok(hash) => {
                self.raw = String::from_utf8_lossy(if self.bom {
                    &bytes[BOM.len()..]
                } else {
                    &bytes
                })
                .into_owned();
                self.ending = scripts::decode(&self.raw).1;
                self.permissions = permissions;
                self.hash = hash;
                self.saved_revision = revision;
                self.trouble = None;
                return Ok(());
            }
            Err(WriteError::Changed) => Trouble::Changed,
            Err(WriteError::Removed) => Trouble::Removed,
            Err(WriteError::Failed(message)) => Trouble::Failed(message),
        };
        self.trouble = Some(trouble.clone());
        Err(trouble)
    }

    /// Writes the text when the file still holds the bytes that this console read.
    pub fn save(&mut self, text: &str, revision: u64) -> Result<(), Trouble> {
        self.write(text, Expect::Hash(self.hash), revision)
    }

    /// Writes the text over the file, whatever the file holds. The user chose it, and
    /// a removed file comes back.
    pub fn overwrite(&mut self, text: &str, revision: u64) -> Result<(), Trouble> {
        self.write(text, Expect::Any, revision)
    }

    /// Reads the file again. The caller replaces the text of the editor, and it passes
    /// the revision of the new editor to `adopt`.
    pub fn reload(&mut self) -> io::Result<String> {
        let read = Self::read(&self.path)?;
        self.hash = read.file.hash;
        self.target = read.file.target;
        self.symlink = read.file.symlink;
        self.permissions = read.file.permissions;
        self.ending = read.file.ending;
        self.raw = read.file.raw;
        self.bom = read.file.bom;
        self.trouble = None;
        Ok(read.text)
    }

    /// Records that the editor holds the text of the file at this revision.
    pub fn adopt(&mut self, revision: u64) {
        self.saved_revision = revision;
    }

    /// The user closes the console without the pending text.
    pub fn discard(&mut self, revision: u64) {
        self.saved_revision = revision;
        self.trouble = None;
    }

    /// Follows a new editor that holds the same text. `dirty` says whether the text
    /// of the new editor is newer than the file.
    pub fn rebase(&mut self, revision: u64, dirty: bool) {
        self.saved_revision = if dirty {
            revision.wrapping_add(1)
        } else {
            revision
        };
    }
}
