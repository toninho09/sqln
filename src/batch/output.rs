//! A pipe that stops waiting after a signal or execution cancellation.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

use crate::exec::StreamControl;

pub(super) struct Output<'a> {
    file: File,
    flags: OFlags,
    stopped: &'a AtomicBool,
    stream_stopped: &'a StreamControl,
}

impl<'a> Output<'a> {
    pub(super) fn new(
        destination: impl AsFd,
        stopped: &'a AtomicBool,
        stream_stopped: &'a StreamControl,
    ) -> io::Result<Self> {
        let file = File::from(rustix::io::dup(destination)?);
        let flags = fcntl_getfl(&file)?;
        fcntl_setfl(&file, flags | OFlags::NONBLOCK)?;
        Ok(Self {
            file,
            flags,
            stopped,
            stream_stopped,
        })
    }
}

impl Write for Output<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            if self.stopped.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "The output stops after a signal.",
                ));
            }
            if self.stream_stopped.is_stopped() {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "The output stops after execution cancellation or SQL timeout.",
                ));
            }
            match self.file.write(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Attempts the final diagnostic without waiting for an error-stream reader.
pub(super) fn report_error(bytes: &[u8]) {
    let stopped = AtomicBool::new(false);
    let control = StreamControl::default();
    if let Ok(mut output) = Output::new(io::stderr(), &stopped, &control) {
        let _ = output.file.write_all(bytes);
    }
}

impl Drop for Output<'_> {
    fn drop(&mut self) {
        let _ = fcntl_setfl(&self.file, self.flags);
    }
}
