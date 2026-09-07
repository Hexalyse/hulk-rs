use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL_CONDENSED;
use comfy_table::{
    Attribute, Cell, CellAlignment, Color, ColumnConstraint, ContentArrangement, Table, Width,
};
use console::{style, Term};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::config::AppConfig;
use crate::stats::{Snapshot, Stats};

#[derive(Clone)]
pub struct Dashboard {
    bar: ProgressBar,
}

impl Dashboard {
    pub fn new() -> Self {
        let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
        bar.set_style(ProgressStyle::with_template("{msg}").expect("progress template is valid"));
        Self { bar }
    }

    pub async fn run_live(
        &self,
        stats: Arc<Stats>,
        config: Arc<AppConfig>,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            self.bar
                .set_message(format_dashboard(&stats.snapshot(), &config, false));
        }
    }

    pub fn note(&self, message: impl AsRef<str>) {
        self.bar.println(message.as_ref());
    }

    pub fn finish(&self) {
        self.bar.finish_and_clear();
    }
}

pub fn print_final(stats: &Stats, config: &AppConfig) {
    println!("{}", format_dashboard(&stats.snapshot(), config, true));
}

fn format_dashboard(snap: &Snapshot, config: &AppConfig, finished: bool) -> String {
    let width = dashboard_width();
    let mut out = String::new();

    out.push_str(&header_block(snap, config, finished, width));
    out.push_str("\n\n");
    out.push_str(&section_title("Requests"));
    out.push('\n');
    push_table(&mut out, requests_table(snap));
    out.push_str(&section_title("Outcomes"));
    out.push('\n');
    push_table(&mut out, outcomes_table(snap, finished));
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

fn header_block(snap: &Snapshot, config: &AppConfig, finished: bool, width: usize) -> String {
    let rps = completed_per_sec(snap);
    let title = if finished {
        style("Stopped").magenta().bold()
    } else {
        style("HULK").cyan().bold()
    };
    let line = format!(
        "{}   {}   {}   {}",
        title,
        style(format_duration(snap.elapsed)).bold(),
        style(format!("{rps:.0} req/s")).cyan(),
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
    table
}

fn outcomes_table(snap: &Snapshot, finished: bool) -> Table {
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
    if finished {
        add_transport_breakdown(&mut table, snap);
    }
    if finished && (snap.status_1xx > 0 || snap.status_other > 0) {
        table.add_row(vec![
            Cell::new("1xx info"),
            value_cell(snap.status_1xx, Tone::Info),
            Cell::new("Other"),
            value_cell(snap.status_other, Tone::Plain),
        ]);
    }
    if finished && snap.build_errors > 0 {
        table.add_row(vec![
            label_cell("Build", Color::Red, true),
            value_cell(snap.build_errors, Tone::Error),
            Cell::new(""),
            Cell::new(""),
        ]);
    }
    table
}

fn add_transport_breakdown(table: &mut Table, snap: &Snapshot) {
    let kinds = [
        ("Connect", snap.connect_errors),
        ("TLS", snap.tls_errors),
        ("Reset", snap.reset_errors),
        ("Timeout", snap.timeout_errors),
        ("Other", snap.other_transport_errors),
    ];
    let present: Vec<(&str, u64)> = kinds.into_iter().filter(|(_, n)| *n > 0).collect();
    if present.len() <= 1 {
        return;
    }
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

fn latency_block(snap: &Snapshot) -> String {
    format!(
        "{}\n  {} {}    {} {}    {} {}    {} {}    {} {}    {} {}\n",
        style("Latency to headers").white().bold().underlined(),
        style("mean").dim(),
        style(format_latency_us(snap.mean_us)).bold(),
        style("min").dim(),
        style(format_latency_us(snap.min_us)).bold(),
        style("p50").dim(),
        style(format_latency_us(snap.p50_us)).bold(),
        style("p95").dim(),
        style(format_latency_us(snap.p95_us)).bold(),
        style("p99").dim(),
        style(format_latency_us(snap.p99_us)).bold(),
        style("max").dim(),
        style(format_latency_us(snap.max_us)).bold(),
    )
}

#[derive(Clone, Copy)]
enum Tone {
    Success,
    Redirect,
    Error,
    Info,
    Plain,
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

fn label_cell(text: &str, color: Color, colored: bool) -> Cell {
    let cell = Cell::new(text);
    if colored {
        cell.fg(color).add_attribute(Attribute::Bold)
    } else {
        cell
    }
}

fn value_cell(n: u64, tone: Tone) -> Cell {
    text_cell(n.to_string(), tone, n > 0).set_alignment(CellAlignment::Right)
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
        .clamp(60, 88)
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
