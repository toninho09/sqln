//! The connection pool of one connection definition.
//! Idle, in use, opening, and cleaning sessions never exceed the limit.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::{Instant, timeout};

use crate::command_log::TraceContext;
use crate::connector::{ConnectError, Connector, ResetOutcome, Session};
use crate::sql::Dialect;

/// The right to use one place of the pool, with a session or without one.
enum Grant {
    Session(Box<dyn Session>),
    Open,
}

struct Waiter {
    id: u64,
    /// Sends the place and the generation of the pool at the moment of the grant.
    grant: oneshot::Sender<(Grant, u64)>,
}

/// A session in the pool, with the moment of its return.
struct Idle {
    session: Box<dyn Session>,
    since: Instant,
}

impl Idle {
    fn now(session: Box<dyn Session>) -> Self {
        Self {
            session,
            since: Instant::now(),
        }
    }
}

#[derive(Default)]
struct State {
    /// The last session to return is the first to leave, so the older sessions age.
    idle: Vec<Idle>,
    /// Places in use, opening, or in cleanup.
    busy: usize,
    waiters: VecDeque<Waiter>,
    next_waiter: u64,
    /// A session that returns from an older generation closes.
    generation: u64,
}

impl State {
    fn total(&self) -> usize {
        self.busy + self.idle.len()
    }
}

struct Inner {
    connector: Arc<dyn Connector>,
    limit: usize,
    /// True when the source keeps its state in one session. The pool holds exactly
    /// one place, and no idle session closes, because a new session starts empty.
    single: bool,
    connect_timeout: Duration,
    state: Mutex<State>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Gives a free place to the first waiter that still waits. The grant carries the
    /// generation of the pool, so a later `close_idle` cannot return an old session.
    fn wake(&self, state: &mut State) {
        while state.total() < self.limit || !state.idle.is_empty() {
            let Some(waiter) = state.waiters.pop_front() else {
                return;
            };
            let grant = state
                .idle
                .pop()
                .map_or(Grant::Open, |idle| Grant::Session(idle.session));
            state.busy += 1;
            if let Err((grant, _)) = waiter.grant.send((grant, state.generation)) {
                state.busy -= 1;
                if let Grant::Session(session) = grant {
                    state.idle.push(Idle::now(session));
                }
            }
        }
    }

    fn return_grant(&self, grant: Grant, generation: u64) {
        let mut state = self.lock();
        state.busy -= 1;
        // A session of an older generation closes, as `Lease::release` decides.
        if let Grant::Session(session) = grant
            && state.generation == generation
        {
            state.idle.push(Idle::now(session));
        }
        self.wake(&mut state);
    }

    /// Removes a session from the count. The drop of the session closes it.
    fn free_place(&self) {
        let mut state = self.lock();
        state.busy -= 1;
        self.wake(&mut state);
    }
}

/// Removes a canceled request from the queue. A place that arrives during the
/// cancellation goes back to the pool.
struct WaitGuard {
    inner: Arc<Inner>,
    id: u64,
    grant: Option<oneshot::Receiver<(Grant, u64)>>,
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        let Some(mut grant) = self.grant.take() else {
            return;
        };
        self.inner
            .lock()
            .waiters
            .retain(|waiter| waiter.id != self.id);
        grant.close();
        if let Ok((grant, generation)) = grant.try_recv() {
            self.inner.return_grant(grant, generation);
        }
    }
}

#[derive(Clone)]
pub struct Pool {
    inner: Arc<Inner>,
}

/// A pool that can close while this reference exists.
pub struct WeakPool(Weak<Inner>);

impl WeakPool {
    pub fn upgrade(&self) -> Option<Pool> {
        self.0.upgrade().map(|inner| Pool { inner })
    }
}

/// One reserved place of the pool, with its session. A drop without `release`
/// closes the session and frees the place, so a canceled future cannot lose a place.
pub struct Lease {
    session: Option<Box<dyn Session>>,
    inner: Arc<Inner>,
    generation: u64,
    released: bool,
}

impl Lease {
    pub fn session(&mut self) -> &mut dyn Session {
        self.session
            .as_deref_mut()
            .expect("a lease holds its session until its end")
    }

    /// Cleans the session and returns it to the pool. The session keeps its
    /// place during the cleanup. A session without a confirmed cleanup closes.
    pub async fn release(mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        if session.reset().await == ResetOutcome::Clean {
            let mut state = self.inner.lock();
            if state.generation != self.generation {
                drop(state);
                self.session = Some(session);
                return;
            }
            state.busy -= 1;
            state.idle.push(Idle::now(session));
            self.inner.wake(&mut state);
            drop(state);
            self.released = true;
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.released {
            self.session = None;
            self.inner.free_place();
        }
    }
}

impl Pool {
    pub fn new(connector: Arc<dyn Connector>, limit: usize, connect_timeout: Duration) -> Self {
        let state = Mutex::new(State::default());
        let single = connector.capabilities().single_session;
        Self {
            inner: Arc::new(Inner {
                connector,
                limit: if single { 1 } else { limit.max(1) },
                single,
                connect_timeout,
                state,
            }),
        }
    }

    pub fn capabilities(&self) -> crate::connector::Capabilities {
        self.inner.connector.capabilities()
    }

    pub fn dialect(&self) -> Dialect {
        self.inner.connector.dialect()
    }

    /// Returns the places in use and the limit.
    pub fn usage(&self) -> (usize, usize) {
        (self.inner.lock().busy, self.inner.limit)
    }

    /// Returns true while the pool holds a session or a place in use.
    pub fn has_sessions(&self) -> bool {
        self.inner.lock().total() > 0
    }

    /// Closes each idle session that waited `limit` or more. Returns the number of
    /// closed sessions and the wait until the next idle session reaches the limit.
    /// A source of one session never closes its idle session, because the state of
    /// the source lives in it.
    pub fn close_expired(&self, limit: Duration) -> (usize, Duration) {
        if self.inner.single {
            return (0, limit);
        }
        let now = Instant::now();
        let mut state = self.inner.lock();
        let (expired, kept): (Vec<Idle>, Vec<Idle>) = std::mem::take(&mut state.idle)
            .into_iter()
            .partition(|idle| now.duration_since(idle.since) >= limit);
        state.idle = kept;
        let next = state
            .idle
            .iter()
            .map(|idle| limit.saturating_sub(now.duration_since(idle.since)))
            .min()
            .unwrap_or(limit);
        self.inner.wake(&mut state);
        drop(state);
        let closed = expired.len();
        drop(expired);
        (closed, next)
    }

    /// Returns a reference that does not keep the pool open.
    pub fn downgrade(&self) -> WeakPool {
        WeakPool(Arc::downgrade(&self.inner))
    }

    /// Closes the idle sessions. A session in use closes when it returns.
    pub fn close_idle(&self) {
        let mut state = self.inner.lock();
        state.generation += 1;
        let idle = std::mem::take(&mut state.idle);
        self.inner.wake(&mut state);
        drop(state);
        drop(idle);
    }

    /// Waits for a place without a time limit. A drop of this future cancels the wait.
    pub async fn acquire(&self) -> Result<Lease, ConnectError> {
        self.acquire_traced(None).await
    }

    pub async fn acquire_traced(&self, trace: Option<TraceContext>) -> Result<Lease, ConnectError> {
        let (grant, generation) = self.reserve().await;
        let mut lease = Lease {
            session: None,
            inner: self.inner.clone(),
            generation,
            released: false,
        };
        if let Grant::Session(mut session) = grant {
            session.set_trace_context(trace.clone());
            // An idle session that stopped gets a replacement, and the user sees no error.
            if session.is_alive().await {
                lease.session = Some(session);
                return Ok(lease);
            }
        }
        // The time limit covers the whole connection, as the driver limit does not.
        let connect = self.inner.connector.connect_traced(trace);
        match timeout(self.inner.connect_timeout, connect).await {
            Ok(session) => lease.session = Some(session?),
            Err(_) => {
                return Err(ConnectError::new(
                    "The connection time limit ended.".to_owned(),
                ));
            }
        }
        Ok(lease)
    }

    /// Reserves a place and reads the generation of the pool under the same lock, so a
    /// `close_idle` in between cannot hide the age of a session.
    async fn reserve(&self) -> (Grant, u64) {
        let (mut guard, answer) = {
            let mut state = self.inner.lock();
            if state.waiters.is_empty() {
                if let Some(idle) = state.idle.pop() {
                    state.busy += 1;
                    return (Grant::Session(idle.session), state.generation);
                }
                if state.total() < self.inner.limit {
                    state.busy += 1;
                    return (Grant::Open, state.generation);
                }
            }
            let id = state.next_waiter;
            state.next_waiter += 1;
            let (grant, answer) = oneshot::channel();
            state.waiters.push_back(Waiter { id, grant });
            (
                WaitGuard {
                    inner: self.inner.clone(),
                    id,
                    grant: None,
                },
                answer,
            )
        };
        guard.grant = Some(answer);
        let received = guard
            .grant
            .as_mut()
            .expect("the guard holds the receiver")
            .await;
        guard.grant = None;
        received.expect("the pool holds each waiter until it sends a place")
    }
}
