use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use hyper::StatusCode;

const RELAXED: Ordering = Ordering::Relaxed;
const LATENCY_SHARDS: usize = 32;
/// One day, in microseconds. Samples above this still update min/max/mean.
const HDR_HIGH_US: u64 = 86_400_000_000;
const STATUS_SLOTS: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Connect,
    Tls,
    Reset,
    Timeout,
    Dns,
    Local,
    Other,
}

#[derive(Debug)]
pub struct Stats {
    start: Instant,
    attempted: AtomicU64,
    in_flight: AtomicU64,
    completed: AtomicU64,
    body_truncated: AtomicU64,
    status_1xx: AtomicU64,
    status_2xx: AtomicU64,
    status_3xx: AtomicU64,
    status_4xx: AtomicU64,
    status_5xx: AtomicU64,
    status_other: AtomicU64,
    status_codes: [AtomicU64; STATUS_SLOTS],
    transport_errors: AtomicU64,
    connect_errors: AtomicU64,
    tls_errors: AtomicU64,
    reset_errors: AtomicU64,
    timeout_errors: AtomicU64,
    dns_errors: AtomicU64,
    local_errors: AtomicU64,
    other_transport_errors: AtomicU64,
    body_errors: AtomicU64,
    body_timeouts: AtomicU64,
    build_errors: AtomicU64,
    header_sum_us: AtomicU64,
    header_count: AtomicU64,
    header_min_us: AtomicU64,
    header_max_us: AtomicU64,
    total_sum_us: AtomicU64,
    total_count: AtomicU64,
    total_min_us: AtomicU64,
    total_max_us: AtomicU64,
    header_hdr: Vec<Mutex<Histogram<u64>>>,
}

#[derive(Debug, Clone, Copy)]
pub struct Snapshot {
    pub elapsed: Duration,
    pub attempted: u64,
    pub in_flight: u64,
    pub completed: u64,
    pub body_truncated: u64,
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
    pub dns_errors: u64,
    pub local_errors: u64,
    pub other_transport_errors: u64,
    pub body_errors: u64,
    pub body_timeouts: u64,
    pub build_errors: u64,
    pub header_count: u64,
    pub header_mean_us: u64,
    pub header_min_us: u64,
    pub header_max_us: u64,
    pub header_p50_us: u64,
    pub header_p95_us: u64,
    pub header_p99_us: u64,
    pub total_count: u64,
    pub total_mean_us: u64,
    pub total_min_us: u64,
    pub total_max_us: u64,
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
            body_truncated: AtomicU64::new(0),
            status_1xx: AtomicU64::new(0),
            status_2xx: AtomicU64::new(0),
            status_3xx: AtomicU64::new(0),
            status_4xx: AtomicU64::new(0),
            status_5xx: AtomicU64::new(0),
            status_other: AtomicU64::new(0),
            status_codes: std::array::from_fn(|_| AtomicU64::new(0)),
            transport_errors: AtomicU64::new(0),
            connect_errors: AtomicU64::new(0),
            tls_errors: AtomicU64::new(0),
            reset_errors: AtomicU64::new(0),
            timeout_errors: AtomicU64::new(0),
            dns_errors: AtomicU64::new(0),
            local_errors: AtomicU64::new(0),
            other_transport_errors: AtomicU64::new(0),
            body_errors: AtomicU64::new(0),
            body_timeouts: AtomicU64::new(0),
            build_errors: AtomicU64::new(0),
            header_sum_us: AtomicU64::new(0),
            header_count: AtomicU64::new(0),
            header_min_us: AtomicU64::new(u64::MAX),
            header_max_us: AtomicU64::new(0),
            total_sum_us: AtomicU64::new(0),
            total_count: AtomicU64::new(0),
            total_min_us: AtomicU64::new(u64::MAX),
            total_max_us: AtomicU64::new(0),
            header_hdr: (0..LATENCY_SHARDS)
                .map(|_| Mutex::new(new_histogram()))
                .collect(),
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
            TransportKind::Dns => self.dns_errors.fetch_add(1, RELAXED),
            TransportKind::Local => self.local_errors.fetch_add(1, RELAXED),
            TransportKind::Other => self.other_transport_errors.fetch_add(1, RELAXED),
        };
    }

    pub fn record_status(&self, status: StatusCode) {
        let code = status.as_u16();
        if (code as usize) < STATUS_SLOTS {
            self.status_codes[code as usize].fetch_add(1, RELAXED);
        }
        match code {
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

    pub fn record_body_timeout(&self) {
        self.body_timeouts.fetch_add(1, RELAXED);
    }

    pub fn record_completed(&self) {
        self.completed.fetch_add(1, RELAXED);
    }

    /// Included in `completed`. The body was longer than the cap, so the connection was not reused.
    pub fn record_truncated(&self) {
        self.body_truncated.fetch_add(1, RELAXED);
    }

    /// Time from send until HTTP headers arrive.
    pub fn record_header_latency(&self, worker_id: usize, latency: Duration) {
        let us = duration_us(latency);
        self.header_sum_us.fetch_add(us, RELAXED);
        self.header_count.fetch_add(1, RELAXED);
        self.header_min_us.fetch_min(us, RELAXED);
        self.header_max_us.fetch_max(us, RELAXED);
        let shard = worker_id % self.header_hdr.len();
        let mut hist = self.header_hdr[shard]
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let sample = us.clamp(1, HDR_HIGH_US);
        let _ = hist.record(sample);
    }

    /// Time from send until the body read finishes (success or cap).
    pub fn record_total_latency(&self, latency: Duration) {
        let us = duration_us(latency);
        self.total_sum_us.fetch_add(us, RELAXED);
        self.total_count.fetch_add(1, RELAXED);
        self.total_min_us.fetch_min(us, RELAXED);
        self.total_max_us.fetch_max(us, RELAXED);
    }

    pub fn status_hits(&self) -> Vec<(u16, u64)> {
        (100..STATUS_SLOTS as u16)
            .filter_map(|code| {
                let n = self.status_codes[code as usize].load(RELAXED);
                (n > 0).then_some((code, n))
            })
            .collect()
    }

    /// Read-only snapshot. Reporting never increments counters.
    ///
    /// After each request settles, `attempted = in_flight + build + transport + completed + body errors + body timeouts`.
    /// `body_truncated` is a subset of `completed`. Status classes include responses whose body is still being read.
    pub fn snapshot(&self) -> Snapshot {
        let header_count = self.header_count.load(RELAXED);
        let total_count = self.total_count.load(RELAXED);
        let header_min = self.header_min_us.load(RELAXED);
        let total_min = self.total_min_us.load(RELAXED);
        let (p50, p95, p99) = self.header_quantiles();

        Snapshot {
            elapsed: self.start.elapsed(),
            attempted: self.attempted.load(RELAXED),
            in_flight: self.in_flight.load(RELAXED),
            completed: self.completed.load(RELAXED),
            body_truncated: self.body_truncated.load(RELAXED),
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
            dns_errors: self.dns_errors.load(RELAXED),
            local_errors: self.local_errors.load(RELAXED),
            other_transport_errors: self.other_transport_errors.load(RELAXED),
            body_errors: self.body_errors.load(RELAXED),
            body_timeouts: self.body_timeouts.load(RELAXED),
            build_errors: self.build_errors.load(RELAXED),
            header_count,
            header_mean_us: mean(self.header_sum_us.load(RELAXED), header_count),
            header_min_us: finite_min(header_min),
            header_max_us: self.header_max_us.load(RELAXED),
            header_p50_us: p50,
            header_p95_us: p95,
            header_p99_us: p99,
            total_count,
            total_mean_us: mean(self.total_sum_us.load(RELAXED), total_count),
            total_min_us: finite_min(total_min),
            total_max_us: self.total_max_us.load(RELAXED),
        }
    }

    fn header_quantiles(&self) -> (u64, u64, u64) {
        let mut acc = new_histogram();
        for shard in &self.header_hdr {
            let hist = shard.lock().unwrap_or_else(|err| err.into_inner());
            if hist.is_empty() {
                continue;
            }
            if acc.add(&*hist).is_err() {
                return (0, 0, 0);
            }
        }
        if acc.is_empty() {
            (0, 0, 0)
        } else {
            (
                acc.value_at_quantile(0.50),
                acc.value_at_quantile(0.95),
                acc.value_at_quantile(0.99),
            )
        }
    }
}

fn new_histogram() -> Histogram<u64> {
    Histogram::new_with_max(HDR_HIGH_US, 3).expect("histogram bounds are valid")
}

fn duration_us(latency: Duration) -> u64 {
    latency.as_micros().min(u128::from(u64::MAX)) as u64
}

fn mean(sum: u64, count: u64) -> u64 {
    sum.checked_div(count).unwrap_or(0)
}

fn finite_min(min_us: u64) -> u64 {
    if min_us == u64::MAX {
        0
    } else {
        min_us
    }
}

pub fn classify_transport(err: &(dyn std::error::Error + 'static)) -> TransportKind {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(err) = current {
        if let Some(kind) = classify_io(err) {
            return kind;
        }
        if let Some(hyper_err) = err.downcast_ref::<hyper::Error>() {
            if hyper_err.is_timeout() {
                return TransportKind::Timeout;
            }
            if hyper_err.is_closed() || hyper_err.is_incomplete_message() || hyper_err.is_canceled()
            {
                return TransportKind::Reset;
            }
        }
        let text = err.to_string().to_ascii_lowercase();
        if text.contains("dns error")
            || text.contains("failed to lookup")
            || text.contains("name or service not known")
            || text.contains("no such host")
        {
            return TransportKind::Dns;
        }
        if text.contains("tls")
            || text.contains("ssl")
            || text.contains("certificate")
            || text.contains("handshake")
        {
            return TransportKind::Tls;
        }
        current = err.source();
    }
    TransportKind::Other
}

fn classify_io(err: &(dyn std::error::Error + 'static)) -> Option<TransportKind> {
    let io = err.downcast_ref::<std::io::Error>()?;
    Some(match io.kind() {
        ErrorKind::TimedOut => TransportKind::Timeout,
        ErrorKind::ConnectionRefused | ErrorKind::NotConnected => TransportKind::Connect,
        ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe
        | ErrorKind::UnexpectedEof => TransportKind::Reset,
        ErrorKind::AddrInUse | ErrorKind::AddrNotAvailable => TransportKind::Local,
        ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => TransportKind::Connect,
        _ => {
            if io.raw_os_error().is_some_and(is_local_resource) {
                TransportKind::Local
            } else {
                return None;
            }
        }
    })
}

fn is_local_resource(code: i32) -> bool {
    matches!(
        code,
        23 | 24 | // ENFILE, EMFILE (Linux and macOS)
        55 | 105 | // ENOBUFS (macOS, Linux)
        10024 | 10055 // WSAEMFILE, WSAENOBUFS
    )
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::*;

    #[derive(Debug)]
    struct Chain {
        message: &'static str,
        source: Option<Box<dyn std::error::Error>>,
    }

    impl fmt::Display for Chain {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.message)
        }
    }

    impl std::error::Error for Chain {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source.as_deref()
        }
    }

    #[test]
    fn settled_requests_leave_the_in_flight_count() {
        let stats = Stats::new();
        {
            let guard = stats.begin_request();
            assert_eq!(stats.snapshot().in_flight, 1);
            stats.record_status(StatusCode::OK);
            stats.record_completed();
            drop(guard);
        }
        let snap = stats.snapshot();
        assert_eq!(snap.attempted, 1);
        assert_eq!(snap.in_flight, 0);
        assert_eq!(snap.completed, 1);
        assert_eq!(snap.status_2xx, 1);
        assert_eq!(
            snap.attempted,
            snap.in_flight
                + snap.build_errors
                + snap.transport_errors
                + snap.completed
                + snap.body_errors
                + snap.body_timeouts
        );
        assert_eq!(stats.status_hits(), vec![(200, 1)]);
    }

    #[test]
    fn header_percentiles_track_recorded_samples() {
        let stats = Stats::new();
        for us in 1..101 {
            stats.record_header_latency(us as usize, Duration::from_micros(us));
        }
        let snap = stats.snapshot();
        assert_eq!(snap.header_count, 100);
        assert_eq!(snap.header_min_us, 1);
        assert_eq!(snap.header_max_us, 100);
        assert!((40..=70).contains(&snap.header_p50_us));
    }

    #[test]
    fn classifies_local_dns_and_reset_errors() {
        let reset = std::io::Error::new(ErrorKind::ConnectionReset, "reset");
        assert_eq!(classify_transport(&reset), TransportKind::Reset);

        let emfile = std::io::Error::from_raw_os_error(24);
        assert_eq!(classify_transport(&emfile), TransportKind::Local);

        let dns = Chain {
            message: "dns error",
            source: Some(Box::new(std::io::Error::other("lookup failed"))),
        };
        assert_eq!(classify_transport(&dns), TransportKind::Dns);

        let tls = Chain {
            message: "tls handshake failed",
            source: None,
        };
        assert_eq!(classify_transport(&tls), TransportKind::Tls);
    }
}
