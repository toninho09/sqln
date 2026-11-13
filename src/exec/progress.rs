//! The counts of a read that the screen shows while the read runs. The connector writes
//! them, and the screen reads them at each frame. One event for each change could fill
//! the event queue, so the counts do not travel as events.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const NO_PERCENT: u8 = u8::MAX;
const NO_LAG: u64 = u64::MAX;
/// The time between two measures of the speed.
const RATE_PERIOD: Duration = Duration::from_secs(1);

/// The reason that ended a read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadEnd {
    /// The read runs, or the grid stopped it. The summary of the grid gives that reason.
    #[default]
    None,
    /// Each partition reached its end.
    End,
    /// The read found the number of matches of `LIMIT`.
    Limit,
    /// The read scanned the number of messages of `SCAN`.
    Scan,
    /// The user or a time limit stopped the read.
    Stopped,
}

impl ReadEnd {
    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::End,
            2 => Self::Limit,
            3 => Self::Scan,
            4 => Self::Stopped,
            _ => Self::None,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::End => 1,
            Self::Limit => 2,
            Self::Scan => 3,
            Self::Stopped => 4,
        }
    }
}

/// The counts of one read.
#[derive(Debug)]
pub struct ReadProgress {
    /// True when a filter selects the messages, so the matches differ from the scan.
    filtered: bool,
    scanned: AtomicU64,
    matched: AtomicU64,
    bytes: AtomicU64,
    percent: AtomicU8,
    lag: AtomicU64,
    end: AtomicU8,
}

/// The counts of a read at one time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub filtered: bool,
    pub scanned: u64,
    pub matched: u64,
    /// The bytes of the keys, the values, and the headers of the scanned messages.
    pub bytes: u64,
    /// The part of a finite read that is done, from 0 to 100.
    pub percent: Option<u8>,
    /// The messages that a continuous read still has to read.
    pub lag: Option<u64>,
    pub end: ReadEnd,
}

impl ReadProgress {
    pub fn new(filtered: bool) -> Self {
        Self {
            filtered,
            scanned: AtomicU64::new(0),
            matched: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            percent: AtomicU8::new(NO_PERCENT),
            lag: AtomicU64::new(NO_LAG),
            end: AtomicU8::new(ReadEnd::None.code()),
        }
    }

    /// Counts one scanned message and its bytes.
    pub fn add_scanned(&self, bytes: u64) {
        self.scanned.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Counts one message that matches the filter.
    pub fn add_matched(&self) {
        self.matched.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_percent(&self, percent: Option<u8>) {
        let value = percent.map_or(NO_PERCENT, |percent| percent.min(100));
        self.percent.store(value, Ordering::Relaxed);
    }

    pub fn set_lag(&self, lag: Option<u64>) {
        let value = lag.map_or(NO_LAG, |lag| lag.min(NO_LAG - 1));
        self.lag.store(value, Ordering::Relaxed);
    }

    pub fn set_end(&self, end: ReadEnd) {
        self.end.store(end.code(), Ordering::Relaxed);
    }

    pub fn scanned(&self) -> u64 {
        self.scanned.load(Ordering::Relaxed)
    }

    pub fn matched(&self) -> u64 {
        self.matched.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        let percent = self.percent.load(Ordering::Relaxed);
        let lag = self.lag.load(Ordering::Relaxed);
        ProgressSnapshot {
            filtered: self.filtered,
            scanned: self.scanned(),
            matched: self.matched(),
            bytes: self.bytes.load(Ordering::Relaxed),
            percent: (percent != NO_PERCENT).then_some(percent),
            lag: (lag != NO_LAG).then_some(lag),
            end: ReadEnd::from_code(self.end.load(Ordering::Relaxed)),
        }
    }
}

/// Returns `part` as a percent of `whole`, from 0 to 100. A `whole` of 0 has no percent.
pub fn percent(part: u64, whole: u64) -> Option<u8> {
    if whole == 0 {
        return None;
    }
    let value = u128::from(part.min(whole)) * 100 / u128::from(whole);
    u8::try_from(value).ok()
}

/// The counts of a read on the screen, with the speed of the read.
#[derive(Debug)]
pub struct ReadMeter {
    progress: Arc<ReadProgress>,
    /// The time and the scanned messages of the last measure.
    sample: Option<(Instant, u64)>,
    rate: Option<u64>,
}

impl ReadMeter {
    pub fn new(progress: Arc<ReadProgress>) -> Self {
        Self {
            progress,
            sample: None,
            rate: None,
        }
    }

    /// Measures the speed of the read. The first speed comes one second after the
    /// first call.
    pub fn sample(&mut self, now: Instant) {
        let scanned = self.progress.scanned();
        let Some((at, before)) = self.sample else {
            self.sample = Some((now, scanned));
            return;
        };
        let time = now.saturating_duration_since(at);
        if time < RATE_PERIOD {
            return;
        }
        let messages = scanned.saturating_sub(before);
        let per_second = u128::from(messages) * 1000 / time.as_millis().max(1);
        self.rate = Some(u64::try_from(per_second).unwrap_or(u64::MAX));
        self.sample = Some((now, scanned));
    }

    /// The scanned messages for each second, in the last measure.
    pub fn rate(&self) -> Option<u64> {
        self.rate
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        self.progress.snapshot()
    }
}
