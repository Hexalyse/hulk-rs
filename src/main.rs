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
use report::Dashboard;
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
        "{} Starting HULK on {} ({} workers)",
        console::style("[*]").cyan().bold(),
        config.target,
        config.max_connections
    );
    if !config.query.fuzz.is_empty() {
        let names: Vec<&str> = config.query.fuzz.iter().map(|p| p.name.as_str()).collect();
        println!(
            "{} Fuzzing query params: {}",
            console::style("[*]").cyan().bold(),
            names.join(", ")
        );
    }
    if let Some(warning) = query::reserialize_warning(&config.target) {
        println!("{} {warning}", console::style("[!]").yellow().bold());
    }

    let client = worker::build_client();
    let dashboard = Dashboard::new();
    let mut join_set = tokio::task::JoinSet::new();
    for worker_id in 0..config.max_connections {
        join_set.spawn(worker::run(
            client.clone(),
            Arc::clone(&config),
            Arc::clone(&stats),
            Arc::clone(&shutdown),
            worker_id,
        ));
    }

    let reporter = tokio::spawn({
        let dashboard = dashboard.clone();
        let stats = Arc::clone(&stats);
        let config = Arc::clone(&config);
        let shutdown = Arc::clone(&shutdown);
        async move { dashboard.run_live(stats, config, shutdown).await }
    });

    let mut panics = 0usize;
    let interrupt = wait_for_interrupt();
    tokio::pin!(interrupt);
    loop {
        tokio::select! {
            _ = &mut interrupt => {
                dashboard.note(format!(
                    "{} Ctrl+C received, stopping workers...",
                    console::style("[*]").cyan().bold()
                ));
                break;
            }
            result = join_set.join_next() => {
                match result {
                    None => break,
                    Some(Err(err)) if err.is_panic() => {
                        panics += 1;
                        dashboard.note(format!(
                            "{} Worker task panicked, stopping...",
                            console::style("[!]").red().bold()
                        ));
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
    dashboard.finish();
    report::print_final(&stats, &config);

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
