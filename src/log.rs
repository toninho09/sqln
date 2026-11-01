//! The optional log of `--log`. It holds kinds of events, never SQL, values, or the
//! text of a database error, because that text can quote a value.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: OnceLock<Mutex<File>> = OnceLock::new();

/// Opens the log. The file is new or grows, and only its owner can read it.
pub fn open(path: &Path) -> io::Result<()> {
    let mut options = File::options();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let _ = FILE.set(Mutex::new(file));
    event("start");
    Ok(())
}

/// Writes one line. A failed write stays silent, because the log is optional.
pub fn event(text: &str) {
    let Some(file) = FILE.get() else { return };
    let Ok(mut file) = file.lock() else { return };
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    let _ = writeln!(file, "{seconds} {text}");
}
