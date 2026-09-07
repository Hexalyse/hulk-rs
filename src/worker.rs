use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use hyper::body::HttpBody as _;
use hyper::{Body, Client, Request};
use hyper_tls::HttpsConnector;
use rand::Rng;

use crate::config::AppConfig;
use crate::query;
use crate::stats::{classify_hyper_error, Stats};

/// Stop reading a response body after this many bytes so workers are not pinned to huge payloads.
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub async fn run(config: Arc<AppConfig>, stats: Arc<Stats>, shutdown: Arc<AtomicBool>) {
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, Body>(https);

    while !shutdown.load(Ordering::Relaxed) {
        send_one(&client, &config, &stats).await;
    }
}

fn randomized_request(config: &AppConfig) -> (String, String, String) {
    let mut rng = rand::thread_rng();
    let url = query::build_request_url(&config.target, &config.query, &mut rng);
    let uri = query::request_target(&url);
    let user_agent = pick(&config.user_agents, &mut rng).to_string();
    let referer = format!(
        "{}{}",
        pick(&config.referers, &mut rng),
        query::random_string(rng.gen_range(5..10), &mut rng)
    );
    (uri, user_agent, referer)
}

async fn send_one(
    client: &Client<HttpsConnector<hyper::client::HttpConnector>, Body>,
    config: &AppConfig,
    stats: &Arc<Stats>,
) {
    let _inflight = stats.begin_request();
    let started = Instant::now();
    let (uri, user_agent, referer) = randomized_request(config);

    let request = match Request::get(&uri)
        .header("User-Agent", user_agent)
        .header("Referer", referer)
        .body(Body::empty())
    {
        Ok(request) => request,
        Err(_) => {
            stats.record_build_error();
            return;
        }
    };

    let mut response = match client.request(request).await {
        Ok(response) => response,
        Err(err) => {
            stats.record_transport_error(classify_hyper_error(&err));
            return;
        }
    };

    let status = response.status();
    stats.record_status(status);
    stats.record_latency(started.elapsed());
    if config.verbose && status.as_u16() >= 400 {
        println!("\n[!] HTTP {}", status);
    }

    match drain_body(response.body_mut()).await {
        Ok(()) => stats.record_completed(),
        Err(_) => stats.record_body_error(),
    }
}

async fn drain_body(body: &mut Body) -> Result<(), hyper::Error> {
    let mut read = 0usize;
    while let Some(chunk) = body.data().await {
        let chunk = chunk?;
        read = read.saturating_add(chunk.len());
        if read >= MAX_BODY_BYTES {
            break;
        }
    }
    Ok(())
}

fn pick<'a>(items: &'a [String], rng: &mut impl Rng) -> &'a str {
    &items[rng.gen_range(0..items.len())]
}
