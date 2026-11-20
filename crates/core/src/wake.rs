//! A signal that wakes the main loop when background work delivers a result.
//!
//! The main loop sleeps until a key, a signal, a timer, or a wake. Each producer that sends
//! a result to the interface calls [`Wake::wake`] after the send. A wake without an installed
//! receiver does nothing, so tests and the batch mode need no loop.

use std::sync::{Arc, PoisonError, RwLock};

type Notify = Box<dyn Fn() + Send + Sync>;

/// A cheap handle that producers clone. The main loop installs the receiver.
#[derive(Clone, Default)]
pub struct Wake(Arc<RwLock<Option<Notify>>>);

impl Wake {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the function that wakes the loop. A new call replaces the previous function.
    pub fn install(&self, notify: impl Fn() + Send + Sync + 'static) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(notify));
    }

    /// Wakes the loop. It never blocks, and many wakes before the loop runs count as one.
    pub fn wake(&self) {
        if let Some(notify) = &*self.0.read().unwrap_or_else(PoisonError::into_inner) {
            notify();
        }
    }
}

impl std::fmt::Debug for Wake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wake")
            .field(
                "installed",
                &self
                    .0
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_some(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_wake_without_a_receiver_does_nothing() {
        Wake::new().wake();
    }

    #[test]
    fn clones_share_the_installed_receiver() {
        let count = Arc::new(AtomicUsize::new(0));
        let wake = Wake::new();
        let producer = wake.clone();
        let seen = count.clone();
        wake.install(move || {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        producer.wake();
        producer.wake();
        assert_eq!(count.load(Ordering::Relaxed), 2);
        wake.install(|| {});
        producer.wake();
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }
}
