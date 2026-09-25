use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL_CONDENSED;
use comfy_table::{
    Attribute, Cell, CellAlignment, Color, ColumnConstraint, ContentArrangement, Table, Width,
};
use console::{style, Term};

use crate::config::AppConfig;
use crate::stats::{Snapshot, Stats};

#[derive(Clone)]
pub struct Dashboard {
    inner: Arc<Mutex<DashboardInner>>,
}

struct DashboardInner {
    term: Term,
    lines: usize,
    last: String,
    interactive: bool,
}

impl Dashboard {
    pub fn new() -> Self {
        let term = Term::stderr();
        let interactive = term.is_term();
        if interactive {
            let _ = term.hide_cursor();
        }
        Self {
            inner: Arc::new(Mutex::new(DashboardInner {
                term,
                lines: 0,
                last: String::new(),
                interactive,
            })),
        }
    }

    pub async fn run_live(
        &self,
        stats: Arc<Stats>,
        config: Arc<AppConfig>,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut rate = RateWindow::default();

        loop {
            interval.tick().await;
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if !self.lock().interactive {
                continue;
            }
            let snap = stats.snapshot();
            let hits = if config.verbose {
                stats.status_hits()
            } else {
                Vec::new()
            };
            let lifetime = completed_per_sec(&snap);
            let window = rate.observe(snap.completed, lifetime);
            self.render(&format_dashboard(
                &snap,
                &hits,
                &config,
                false,
                Some(window),
            ));
        }
    }

    pub fn note(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        let inner = self.lock();
        if inner.interactive {
            let _ = inner.term.clear_last_lines(inner.lines);
        }
        let _ = inner.term.write_line(message);
        if inner.interactive && inner.lines > 0 {
            let _ = inner.term.write_str(&inner.last);
        }
    }

    pub fn finish(&self) {
        let mut inner = self.lock();
        if inner.interactive {
            let _ = inner.term.clear_last_lines(inner.lines);
            let _ = inner.term.show_cursor();
            inner.lines = 0;
            inner.last.clear();
        }
    }

    fn render(&self, text: &str) {
        let mut inner = self.lock();
        if !inner.interactive {
            return;
        }
        let _ = inner.term.clear_last_lines(inner.lines);
        let _ = inner.term.write_str(text);
        inner.lines = text.lines().count();
        inner.last = text.to_string();
    }

    fn lock(&self) -> MutexGuard<'_, DashboardInner> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }
}

pub fn print_final(stats: &Stats, config: &AppConfig) {
    let snap = stats.snapshot();
    let hits = if config.verbose {
        stats.status_hits()
    } else {
        Vec::new()
    };
    println!("{}", format_dashboard(&snap, &hits, config, true, None));
}

fn format_dashboard(
    snap: &Snapshot,
    hits: &[(u16, u64)],
    config: &AppConfig,
    finished: bool,
    window_rps: Option<f64>,
) -> String {
    let width = dashboard_width();
    let mut out = String::new();

    out.push_str(&header_block(snap, config, finished, width, window_rps));
    out.push_str("\n\n");
    out.push_str(&section_title("Requests"));
    out.push('\n');
    push_table(&mut out, requests_table(snap));
    out.push_str(&section_title("Outcomes"));
    out.push('\n');
    push_table(&mut out, outcomes_table(snap));
    if config.verbose && !hits.is_empty() {
        out.push_str(&section_title("Status codes"));
        out.push('\n');
        push_table(&mut out, status_table(hits));
    }
    out.push_str(&latency_block(snap));
    out
}

fn push_table(out: &mut String, table: Table) {
    let rendered = table.to_string();
    out.push_str(&rendered);
    if !rendered.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
}

fn header_block(
    snap: &Snapshot,
    config: &AppConfig,
    finished: bool,
    width: usize,
    window_rps: Option<f64>,
) -> String {
    let lifetime = completed_per_sec(snap);
    let title = if finished {
        style("Stopped").magenta().bold()
    } else {
        style("HULK").cyan().bold()
    };
    let rate = match window_rps {
        Some(window) => format!(
            "{}   {}",
            style(format!("{window:.0} req/s")).cyan(),
            style(format!("avg {lifetime:.0}")).dim(),
        ),
        None => style(format!("{lifetime:.0} req/s")).cyan().to_string(),
    };
    let line = format!(
        "{}   {}   {}   {}",
        title,
        style(format_duration(snap.elapsed)).bold(),
        rate,
        style(format!("{} workers", config.max_connections)).dim(),
    );
    let target = console::truncate_str(config.target.as_str(), width.saturating_sub(2), "…");
    format!("{line}\n{}", style(target).dim())
}

fn section_title(name: &str) -> String {
    style(name).white().bold().underlined().to_string()
}

fn requests_table(snap: &Snapshot) -> Table {
    let mut table = pair_table();
    table.add_row(vec![
        Cell::new("Completed"),
        value_cell(snap.completed, Tone::Plain),
        Cell::new("Attempted"),
        value_cell(snap.attempted, Tone::Plain),
    ]);
    table.add_row(vec![
        Cell::new("In flight"),
        value_cell(snap.in_flight, Tone::Plain),
        Cell::new("Statuses"),
        value_cell(reply_count(snap), Tone::Plain),
    ]);
    if snap.build_errors > 0 {
        table.add_row(vec![
            label_cell("Build", Color::Red, true),
            value_cell(snap.build_errors, Tone::Error),
            Cell::new(""),
            Cell::new(""),
        ]);
    }
    table
}

fn reply_count(snap: &Snapshot) -> u64 {
    snap.status_1xx
        + snap.status_2xx
        + snap.status_3xx
        + snap.status_4xx
        + snap.status_5xx
        + snap.status_other
}

fn outcomes_table(snap: &Snapshot) -> Table {
    let mut table = pair_table();
    table.add_row(vec![
        label_cell("2xx success", Color::Green, true),
        value_cell(snap.status_2xx, Tone::Success),
        label_cell("3xx redirect", Color::Yellow, true),
        value_cell(snap.status_3xx, Tone::Redirect),
    ]);
    table.add_row(vec![
        label_cell("4xx client", Color::Red, true),
        value_cell(snap.status_4xx, Tone::Error),
        label_cell("5xx server", Color::Red, true),
        value_cell(snap.status_5xx, Tone::Error),
    ]);
    table.add_row(vec![
        label_cell("Transport", Color::Red, true),
        value_cell(snap.transport_errors, Tone::Error),
        label_cell("Body-read", Color::Red, true),
        value_cell(snap.body_errors, Tone::Error),
    ]);
    if snap.body_timeouts > 0 || snap.body_truncated > 0 {
        table.add_row(vec![
            label_cell("Body timeout", Color::Red, true),
            value_cell(snap.body_timeouts, Tone::Error),
            label_cell("Truncated", Color::Yellow, true),
            value_cell(snap.body_truncated, Tone::Redirect),
        ]);
    }
    if snap.status_1xx > 0 || snap.status_other > 0 {
        table.add_row(vec![
            Cell::new("1xx info"),
            value_cell(snap.status_1xx, Tone::Info),
            Cell::new("Other"),
            value_cell(snap.status_other, Tone::Plain),
        ]);
    }
    add_transport_breakdown(&mut table, snap);
    table
}

fn add_transport_breakdown(table: &mut Table, snap: &Snapshot) {
    if snap.transport_errors == 0 {
        return;
    }
    let kinds = [
        ("Connect", snap.connect_errors),
        ("TLS", snap.tls_errors),
        ("Reset", snap.reset_errors),
        ("Timeout", snap.timeout_errors),
        ("DNS", snap.dns_errors),
        ("Local", snap.local_errors),
        ("Other", snap.other_transport_errors),
    ];
    let present: Vec<(&str, u64)> = kinds.into_iter().filter(|(_, n)| *n > 0).collect();
    for pair in present.chunks(2) {
        match pair {
            [(left, left_n), (right, right_n)] => {
                table.add_row(vec![
                    label_cell(left, Color::Red, true),
                    value_cell(*left_n, Tone::Error),
                    label_cell(right, Color::Red, true),
                    value_cell(*right_n, Tone::Error),
                ]);
            }
            [(left, left_n)] => {
                table.add_row(vec![
                    label_cell(left, Color::Red, true),
                    value_cell(*left_n, Tone::Error),
                    Cell::new(""),
                    Cell::new(""),
                ]);
            }
            _ => {}
        }
    }
}

fn status_table(hits: &[(u16, u64)]) -> Table {
    let mut table = pair_table();
    for pair in hits.chunks(2) {
        match pair {
            [(left, left_n), (right, right_n)] => {
                table.add_row(vec![
                    status_label(*left),
                    value_cell(*left_n, tone_for_status(*left)),
                    status_label(*right),
                    value_cell(*right_n, tone_for_status(*right)),
                ]);
            }
            [(left, left_n)] => {
                table.add_row(vec![
                    status_label(*left),
                    value_cell(*left_n, tone_for_status(*left)),
                    Cell::new(""),
                    Cell::new(""),
                ]);
            }
            _ => {}
        }
    }
    table
}

fn latency_block(snap: &Snapshot) -> String {
    let headers = snap.header_count > 0;
    let total = snap.total_count > 0;
    let mut out = String::new();
    out.push_str(&section_title("Latency to headers"));
    out.push('\n');
    let mut header = metric_table();
    header.add_row(vec![
        dim_cell("mean"),
        latency_cell(snap.header_mean_us, headers),
        dim_cell("min"),
        latency_cell(snap.header_min_us, headers),
        dim_cell("p50"),
        latency_cell(snap.header_p50_us, headers),
    ]);
    header.add_row(vec![
        dim_cell("p95"),
        latency_cell(snap.header_p95_us, headers),
        dim_cell("p99"),
        latency_cell(snap.header_p99_us, headers),
        dim_cell("max"),
        latency_cell(snap.header_max_us, headers),
    ]);
    push_table(&mut out, header);
    out.push_str(&section_title("Time to complete"));
    out.push('\n');
    let mut done = metric_table();
    done.add_row(vec![
        dim_cell("mean"),
        latency_cell(snap.total_mean_us, total),
        dim_cell("min"),
        latency_cell(snap.total_min_us, total),
        dim_cell("max"),
        latency_cell(snap.total_max_us, total),
    ]);
    push_table(&mut out, done);
    out
}

#[derive(Clone, Copy)]
enum Tone {
    Success,
    Redirect,
    Error,
    Info,
    Plain,
}

fn tone_for_status(code: u16) -> Tone {
    match code {
        200..=299 => Tone::Success,
        300..=399 => Tone::Redirect,
        400..=599 => Tone::Error,
        100..=199 => Tone::Info,
        _ => Tone::Plain,
    }
}

fn status_label(code: u16) -> Cell {
    let (color, bold) = match code {
        200..=299 => (Color::Green, true),
        300..=399 => (Color::Yellow, true),
        400..=599 => (Color::Red, true),
        _ => (Color::Cyan, false),
    };
    label_cell(&code.to_string(), color, bold)
}

fn base_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL_CONDENSED)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(dashboard_width() as u16);
    if !console::colors_enabled() {
        table.force_no_tty();
    }
    table
}

fn pair_table() -> Table {
    let mut table = base_table();
    table.set_content_arrangement(ContentArrangement::Disabled);
    table.set_constraints(vec![
        ColumnConstraint::LowerBoundary(Width::Fixed(13)),
        ColumnConstraint::Absolute(Width::Fixed(10)),
        ColumnConstraint::LowerBoundary(Width::Fixed(14)),
        ColumnConstraint::Absolute(Width::Fixed(10)),
    ]);
    table
}

fn metric_table() -> Table {
    let mut table = base_table();
    table.set_content_arrangement(ContentArrangement::Disabled);
    table.set_constraints(vec![
        ColumnConstraint::Absolute(Width::Fixed(6)),
        ColumnConstraint::LowerBoundary(Width::Fixed(8)),
        ColumnConstraint::Absolute(Width::Fixed(6)),
        ColumnConstraint::LowerBoundary(Width::Fixed(8)),
        ColumnConstraint::Absolute(Width::Fixed(6)),
        ColumnConstraint::LowerBoundary(Width::Fixed(8)),
    ]);
    table
}

fn label_cell(text: &str, color: Color, colored: bool) -> Cell {
    let cell = Cell::new(text);
    if colored {
        cell.fg(color).add_attribute(Attribute::Bold)
    } else {
        cell
    }
}

fn dim_cell(text: &str) -> Cell {
    Cell::new(text).fg(Color::DarkGrey)
}

fn value_cell(n: u64, tone: Tone) -> Cell {
    text_cell(n.to_string(), tone, n > 0).set_alignment(CellAlignment::Right)
}

fn latency_cell(us: u64, has_samples: bool) -> Cell {
    let text = if has_samples {
        format_latency_us(us)
    } else {
        "n/a".to_string()
    };
    text_cell(text, Tone::Plain, has_samples)
}

fn text_cell(text: impl ToString, tone: Tone, active: bool) -> Cell {
    let cell = Cell::new(text.to_string());
    if !active {
        return cell.fg(Color::DarkGrey);
    }
    match tone {
        Tone::Success => cell.fg(Color::Green).add_attribute(Attribute::Bold),
        Tone::Redirect => cell.fg(Color::Yellow).add_attribute(Attribute::Bold),
        Tone::Error => cell.fg(Color::Red).add_attribute(Attribute::Bold),
        Tone::Info => cell.fg(Color::Cyan),
        Tone::Plain => cell,
    }
}

fn dashboard_width() -> usize {
    Term::stderr()
        .size_checked()
        .map(|(_, cols)| cols as usize)
        .unwrap_or(80)
        .clamp(60, 120)
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
        "0µs".to_string()
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

#[derive(Default)]
struct RateWindow {
    samples: Vec<(Instant, u64)>,
}

impl RateWindow {
    fn observe(&mut self, completed: u64, lifetime: f64) -> f64 {
        let now = Instant::now();
        self.samples.push((now, completed));
        let cutoff = now - Duration::from_secs(1);
        let keep_from = self
            .samples
            .iter()
            .position(|(at, _)| *at >= cutoff)
            .unwrap_or(self.samples.len().saturating_sub(1));
        if keep_from > 0 {
            self.samples.drain(..keep_from);
        }
        let (started, then) = self.samples[0];
        rate_over(
            then,
            completed,
            now.saturating_duration_since(started),
            lifetime,
        )
    }
}

fn rate_over(then: u64, now_count: u64, dt: Duration, lifetime: f64) -> f64 {
    let secs = dt.as_secs_f64();
    if secs < 0.25 {
        lifetime
    } else {
        now_count.saturating_sub(then) as f64 / secs
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use url::Url;

    use crate::query::QueryPlan;
    use crate::stats::Stats;

    use super::*;

    fn config(verbose: bool) -> AppConfig {
        AppConfig {
            target: Url::parse("http://example.com/").unwrap(),
            max_connections: 4,
            verbose,
            user_agents: Arc::<[String]>::from(vec!["ua".to_string()]),
            referers: Arc::<[String]>::from(vec!["https://example.com/".to_string()]),
            referers_exact: false,
            query: QueryPlan {
                fuzz: Vec::new(),
                cache_bust: true,
                default_length: 10,
            },
            header_timeout: Duration::from_secs(30),
            timeout: Duration::from_secs(60),
            max_body: 1024,
        }
    }

    #[test]
    fn idle_report_marks_latency_missing_and_shows_in_flight() {
        let stats = Stats::new();
        let text = format_dashboard(&stats.snapshot(), &[], &config(false), true, None);
        assert!(text.contains("In flight"));
        assert!(text.contains("n/a"));
        assert!(!text.contains("0µs"));
    }

    #[test]
    fn verbose_report_lists_exact_status_codes() {
        let stats = Stats::new();
        stats.record_status(hyper::StatusCode::NOT_FOUND);
        stats.record_status(hyper::StatusCode::OK);
        let hits = stats.status_hits();
        let text = format_dashboard(&stats.snapshot(), &hits, &config(true), true, None);
        assert!(text.contains("200"));
        assert!(text.contains("404"));
    }

    #[test]
    fn short_windows_use_the_lifetime_rate() {
        assert_eq!(rate_over(0, 10, Duration::from_millis(100), 4.0), 4.0);
        let rate = rate_over(10, 40, Duration::from_millis(500), 1.0);
        assert!((rate - 60.0).abs() < 0.01);
    }

    #[test]
    fn zero_microseconds_is_not_milliseconds() {
        assert_eq!(format_latency_us(0), "0µs");
    }
}
