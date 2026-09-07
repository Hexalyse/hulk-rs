use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyper::StatusCode;

const RELAXED: Ordering = Ordering::Relaxed;

/// Upper bounds in microseconds. Samples above the last bound go into `latency_overflow`.
const LATENCY_BOUNDS_US: &[u64] = &[
    500,
    1_000,
    2_000,
    5_000,
    10_000,
    20_000,
    50_000,
    100_000,
    200_000,
    500_000,
    1_000_000,
    2_000_000,
    5_000_000,
    10_000_000,
    15_000_000,
    20_000_000,
    30_000_000,
    45_000_000,
    60_000_000,
    90_000_000,
    120_000_000,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Connect,
    Tls,
    Reset,
    Timeout,
    Other,
}

#[derive(Debug)]
pub struct Stats {
    start: Instant,
    attempted: AtomicU64,
    in_flight: AtomicU64,
    completed: AtomicU64,
    status_1xx: AtomicU64,
    status_2xx: AtomicU64,
    status_3xx: AtomicU64,
    status_4xx: AtomicU64,
    status_5xx: AtomicU64,
    status_other: AtomicU64,
    transport_errors: AtomicU64,
    connect_errors: AtomicU64,
    tls_errors: AtomicU64,
    reset_errors: AtomicU64,
    timeout_errors: AtomicU64,
    other_transport_errors: AtomicU64,
    body_errors: AtomicU64,
    build_errors: AtomicU64,
    latency_sum_us: AtomicU64,
    latency_count: AtomicU64,
    latency_min_us: AtomicU64,
    latency_max_us: AtomicU64,
    latency_overflow: AtomicU64,
    latency_buckets: [AtomicU64; LATENCY_BOUNDS_US.len()],
}

#[derive(Debug, Clone, Copy)]
pub struct Snapshot {
    pub elapsed: Duration,
    pub attempted: u64,
    pub in_flight: u64,
    pub completed: u64,
    pub status_1xx: u64,
    pub status_2xx: u64,
    pub status_3xx: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
    pub status_other: u64,
    pub transport_errors: u64,
    pub connect_errors: u64,
    pub tls_errors: u64,
    pub reset_errors: u64,
    pub timeout_errors: u64,
    pub other_transport_errors: u64,
    pub body_errors: u64,
    pub build_errors: u64,
    pub mean_us: u64,
    pub min_us: u64,
    pub max_us: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
}

pub struct InflightGuard {
    stats: Arc<Stats>,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.stats.in_flight.fetch_sub(1, RELAXED);
    }
}

impl Stats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            start: Instant::now(),
            attempted: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            status_1xx: AtomicU64::new(0),
            status_2xx: AtomicU64::new(0),
            status_3xx: AtomicU64::new(0),
            status_4xx: AtomicU64::new(0),
            status_5xx: AtomicU64::new(0),
            status_other: AtomicU64::new(0),
            transport_errors: AtomicU64::new(0),
            connect_errors: AtomicU64::new(0),
            tls_errors: AtomicU64::new(0),
            reset_errors: AtomicU64::new(0),
            timeout_errors: AtomicU64::new(0),
            other_transport_errors: AtomicU64::new(0),
            body_errors: AtomicU64::new(0),
            build_errors: AtomicU64::new(0),
            latency_sum_us: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            latency_min_us: AtomicU64::new(u64::MAX),
            latency_max_us: AtomicU64::new(0),
            latency_overflow: AtomicU64::new(0),
            latency_buckets: std::array::from_fn(|_| AtomicU64::new(0)),
        })
    }

    pub fn begin_request(self: &Arc<Self>) -> InflightGuard {
        self.attempted.fetch_add(1, RELAXED);
        self.in_flight.fetch_add(1, RELAXED);
        InflightGuard {
            stats: Arc::clone(self),
        }
    }

    pub fn record_build_error(&self) {
        self.build_errors.fetch_add(1, RELAXED);
    }

    pub fn record_transport_error(&self, kind: TransportKind) {
        self.transport_errors.fetch_add(1, RELAXED);
        match kind {
            TransportKind::Connect => self.connect_errors.fetch_add(1, RELAXED),
            TransportKind::Tls => self.tls_errors.fetch_add(1, RELAXED),
            TransportKind::Reset => self.reset_errors.fetch_add(1, RELAXED),
            TransportKind::Timeout => self.timeout_errors.fetch_add(1, RELAXED),
            TransportKind::Other => self.other_transport_errors.fetch_add(1, RELAXED),
        };
    }

    pub fn record_status(&self, status: StatusCode) {
        match status.as_u16() {
            100..=199 => self.status_1xx.fetch_add(1, RELAXED),
            200..=299 => self.status_2xx.fetch_add(1, RELAXED),
            300..=399 => self.status_3xx.fetch_add(1, RELAXED),
            400..=499 => self.status_4xx.fetch_add(1, RELAXED),
            500..=599 => self.status_5xx.fetch_add(1, RELAXED),
            _ => self.status_other.fetch_add(1, RELAXED),
        };
    }

    pub fn record_body_error(&self) {
        self.body_errors.fetch_add(1, RELAXED);
    }

    pub fn record_completed(&self) {
        self.completed.fetch_add(1, RELAXED);
    }

    /// Time from send until HTTP headers arrive (not including body drain).
    pub fn record_latency(&self, latency: Duration) {
        let us = latency.as_micros().min(u128::from(u64::MAX)) as u64;
        self.latency_sum_us.fetch_add(us, RELAXED);
        self.latency_count.fetch_add(1, RELAXED);
        self.latency_min_us.fetch_min(us, RELAXED);
        self.latency_max_us.fetch_max(us, RELAXED);
        match bucket_index(us) {
            Some(i) => {
                self.latency_buckets[i].fetch_add(1, RELAXED);
            }
            None => {
                self.latency_overflow.fetch_add(1, RELAXED);
            }
        }
    }

    /// Read-only snapshot. Reporting never increments counters.
    pub fn snapshot(&self) -> Snapshot {
        let latency_count = self.latency_count.load(RELAXED);
        let mean_us = if latency_count == 0 {
            0
        } else {
            self.latency_sum_us.load(RELAXED) / latency_count
        };
        let min_us = self.latency_min_us.load(RELAXED);
        let max_us = self.latency_max_us.load(RELAXED);
        let buckets: [u64; LATENCY_BOUNDS_US.len()] =
            std::array::from_fn(|i| self.latency_buckets[i].load(RELAXED));
        let overflow = self.latency_overflow.load(RELAXED);

        Snapshot {
            elapsed: self.start.elapsed(),
            attempted: self.attempted.load(RELAXED),
            in_flight: self.in_flight.load(RELAXED),
            completed: self.completed.load(RELAXED),
            status_1xx: self.status_1xx.load(RELAXED),
            status_2xx: self.status_2xx.load(RELAXED),
            status_3xx: self.status_3xx.load(RELAXED),
            status_4xx: self.status_4xx.load(RELAXED),
            status_5xx: self.status_5xx.load(RELAXED),
            status_other: self.status_other.load(RELAXED),
            transport_errors: self.transport_errors.load(RELAXED),
            connect_errors: self.connect_errors.load(RELAXED),
            tls_errors: self.tls_errors.load(RELAXED),
            reset_errors: self.reset_errors.load(RELAXED),
            timeout_errors: self.timeout_errors.load(RELAXED),
            other_transport_errors: self.other_transport_errors.load(RELAXED),
            body_errors: self.body_errors.load(RELAXED),
            build_errors: self.build_errors.load(RELAXED),
            mean_us,
            min_us: if min_us == u64::MAX { 0 } else { min_us },
            max_us,
            p50_us: percentile_us(&buckets, overflow, max_us, 50.0),
            p95_us: percentile_us(&buckets, overflow, max_us, 95.0),
            p99_us: percentile_us(&buckets, overflow, max_us, 99.0),
        }
    }
}

fn bucket_index(us: u64) -> Option<usize> {
    LATENCY_BOUNDS_US.iter().position(|bound| us <= *bound)
}

fn percentile_us(buckets: &[u64], overflow: u64, max_us: u64, p: f64) -> u64 {
    let total: u64 = buckets.iter().sum::<u64>().saturating_add(overflow);
    if total == 0 {
        return 0;
    }
    let rank = ((p / 100.0) * total as f64).ceil().max(1.0) as u64;
    let mut acc = 0;
    for (i, count) in buckets.iter().enumerate() {
        acc += count;
        if acc >= rank {
            return LATENCY_BOUNDS_US[i];
        }
    }
    max_us
}

pub fn classify_hyper_error(err: &hyper::Error) -> TransportKind {
    if err.is_timeout() {
        return TransportKind::Timeout;
    }
    if err.is_connect() {
        if cause_looks_like_tls(err) {
            return TransportKind::Tls;
        }
        return TransportKind::Connect;
    }
    if err.is_closed() || err.is_incomplete_message() {
        return TransportKind::Reset;
    }
    if cause_looks_like_tls(err) {
        return TransportKind::Tls;
    }
    TransportKind::Other
}

fn cause_looks_like_tls(err: &dyn std::error::Error) -> bool {
    let mut current: Option<&dyn std::error::Error> = Some(err);
    while let Some(e) = current {
        let text = e.to_string().to_ascii_lowercase();
        if text.contains("tls")
            || text.contains("ssl")
            || text.contains("certificate")
            || text.contains("handshake")
        {
            return true;
        }
        current = e.source();
    }
    false
}
