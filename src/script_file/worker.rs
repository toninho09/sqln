//! One bounded worker for script writes, separate from credential operations.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};

use super::{SaveReply as Reply, SaveRequest};
type Job = (SaveRequest, mpsc::Sender<Reply>);

pub(crate) struct SaveWorker {
    sender: Option<mpsc::SyncSender<Job>>,
    thread: Option<JoinHandle<()>>,
    busy: Arc<AtomicBool>,
}

impl SaveWorker {
    pub fn new() -> Self {
        Self::with_hook(|| {})
    }

    fn with_hook(mut before: impl FnMut() + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let busy = Arc::new(AtomicBool::new(false));
        let working = busy.clone();
        let thread = thread::spawn(move || {
            while let Ok((request, reply)) = receiver.recv() {
                before();
                let result = request.run();
                let _ = reply.send(result);
                working.store(false, Ordering::Release);
            }
        });
        Self {
            sender: Some(sender),
            thread: Some(thread),
            busy,
        }
    }

    #[cfg(test)]
    pub(crate) fn gated() -> (Self, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (started, start) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let mut first = true;
        let worker = Self::with_hook(move || {
            if first {
                first = false;
                let _ = started.send(());
                let _ = wait.recv();
            }
        });
        (worker, start, release)
    }

    pub fn ready(&self) -> bool {
        !self.busy.load(Ordering::Acquire)
    }

    pub(super) fn submit(&self, request: SaveRequest) -> mpsc::Receiver<Reply> {
        let (sender, receiver) = mpsc::channel();
        self.busy.store(true, Ordering::Release);
        if self
            .sender
            .as_ref()
            .unwrap()
            .send((request, sender))
            .is_err()
        {
            self.busy.store(false, Ordering::Release);
        }
        receiver
    }
}

impl Drop for SaveWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(super) struct PendingSave {
    pub revision: u64,
    pub metadata_name: Option<String>,
    pub receiver: Option<mpsc::Receiver<Reply>>,
}

impl Drop for PendingSave {
    fn drop(&mut self) {
        // Keep the script lock until the worker stops using this file.
        if let Some(receiver) = self.receiver.take() {
            let _ = receiver.recv();
        }
    }
}
