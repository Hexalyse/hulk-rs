use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::stats::{Snapshot, Stats};

const LIVE_WIDTH: usize = 88;

pub async fn run_live(stats: Arc<Stats>, shutdown: Arc<AtomicBool>) {
    enable_ansi();
    let interactive = io::stdout().is_terminal();
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut first = true;

    loop {
        interval.tick().await;
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        print_live(&stats.snapshot(), interactive, &mut first);
    }
}

fn print_live(snap: &Snapshot, interactive: bool, first: &mut bool) {
    let lines = live_lines(snap);
    if interactive && !*first {
        print!("\x1b[{}A", lines.len());
    }
    *first = false;
    for line in &lines {
        println!("{line:<LIVE_WIDTH$}");
    }
    let _ = io::stdout().flush();
}

fn live_lines(snap: &Snapshot) -> Vec<String> {
    let rps = completed_per_sec(snap);
    vec![
        format!(
            "[*] {elapsed}   {rps:.0} req/s",
            elapsed = format_duration(snap.elapsed),
        ),
        format!(
            "    done {:<12} attempted {:<12} in-flight {}",
            snap.completed, snap.attempted, snap.in_flight
        ),
        format!(
            "    status     2xx {:<10} 3xx {:<10} 4xx {:<10} 5xx {}",
            snap.status_2xx, snap.status_3xx, snap.status_4xx, snap.status_5xx
        ),
        format!(
            "    errors     transport {:<7} body-read {}",
            snap.transport_errors, snap.body_errors
        ),
        format!(
            "    latency    mean {}   p50 {}   p95 {}   p99 {}   max {}",
            format_latency_us(snap.mean_us),
            format_latency_us(snap.p50_us),
            format_latency_us(snap.p95_us),
            format_latency_us(snap.p99_us),
            format_latency_us(snap.max_us),
        ),
    ]
}

pub fn print_final(stats: &Stats) {
    let snap = stats.snapshot();
    let rps = completed_per_sec(&snap);
    println!();
    println!("[*] Stopped after {}", format_duration(snap.elapsed));
    println!("[*] Attempted:          {}", snap.attempted);
    println!("[*] In-flight:          {}", snap.in_flight);
    println!(
        "[*] Completed:          {}  ({:.0} req/s)",
        snap.completed, rps
    );
    println!(
        "[*] Status:             1xx={}  2xx={}  3xx={}  4xx={}  5xx={}  other={}",
        snap.status_1xx,
        snap.status_2xx,
        snap.status_3xx,
        snap.status_4xx,
        snap.status_5xx,
        snap.status_other
    );
    println!(
        "[*] Transport errors:   {}  (no HTTP response: connect={}  tls={}  reset={}  timeout={}  other={})",
        snap.transport_errors,
        snap.connect_errors,
        snap.tls_errors,
        snap.reset_errors,
        snap.timeout_errors,
        snap.other_transport_errors
    );
    println!(
        "[*] Body-read errors:   {}  (HTTP status received, then the body failed)",
        snap.body_errors
    );
    if snap.build_errors > 0 {
        println!("[*] Build errors:       {}", snap.build_errors);
    }
    println!(
        "[*] Latency (headers):  mean {}  min {}  p50 {}  p95 {}  p99 {}  max {}",
        format_latency_us(snap.mean_us),
        format_latency_us(snap.min_us),
        format_latency_us(snap.p50_us),
        format_latency_us(snap.p95_us),
        format_latency_us(snap.p99_us),
        format_latency_us(snap.max_us),
    );
}

fn completed_per_sec(snap: &Snapshot) -> f64 {
    let secs = snap.elapsed.as_secs_f64();
    if secs <= 0.0 {
        0.0
    } else {
        snap.completed as f64 / secs
    }
}

pub fn format_latency_us(us: u64) -> String {
    if us == 0 {
        "0ms".to_string()
    } else if us < 1_000 {
        format!("{us}µs")
    } else if us < 10_000 {
        format!("{:.1}ms", us as f64 / 1_000.0)
    } else if us < 1_000_000 {
        format!("{:.0}ms", us as f64 / 1_000.0)
    } else {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    }
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    if secs < 60 {
        format!("{secs}.{millis:03}s")
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!(
            "{}h{:02}m{:02}s",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    }
}

fn enable_ansi() {
    #[cfg(windows)]
    {
        const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetStdHandle(n: u32) -> isize;
            fn GetConsoleMode(h: isize, mode: *mut u32) -> i32;
            fn SetConsoleMode(h: isize, mode: u32) -> i32;
        }
        unsafe {
            let handle = GetStdHandle((-11i32) as u32);
            if handle == 0 || handle == -1 {
                return;
            }
            let mut mode = 0u32;
            if GetConsoleMode(handle, &mut mode) != 0 {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}
