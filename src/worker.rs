use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::{body::Incoming, Request};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::config::AppConfig;
use crate::query::{self, format_referer};
use crate::stats::{classify_transport, Stats, TransportKind};

pub type HulkClient = Client<HttpsConnector<HttpConnector>, Empty<Bytes>>;

pub fn build_client() -> HulkClient {
    let https = HttpsConnector::new();
    Client::builder(TokioExecutor::new())
        .pool_timer(TokioTimer::default())
        .build(https)
}

pub async fn run(
    client: HulkClient,
    config: Arc<AppConfig>,
    stats: Arc<Stats>,
    shutdown: Arc<AtomicBool>,
    worker_id: usize,
) {
    let mut rng = StdRng::from_entropy();

    while !shutdown.load(Ordering::Relaxed) {
        send_one(&client, &config, &stats, &mut rng, worker_id).await;
    }
}

fn randomized_request(config: &AppConfig, rng: &mut StdRng) -> (String, String, String) {
    let url = query::build_request_url(&config.target, &config.query, rng);
    let uri = query::request_target(&url);
    let user_agent = pick(&config.user_agents, rng).to_string();
    let referer = format_referer(pick(&config.referers, rng), config.referers_exact, rng);
    (uri, user_agent, referer)
}

async fn send_one(
    client: &HulkClient,
    config: &AppConfig,
    stats: &Arc<Stats>,
    rng: &mut StdRng,
    worker_id: usize,
) {
    let _inflight = stats.begin_request();
    let started = Instant::now();
    let (uri, user_agent, referer) = randomized_request(config, rng);

    let request = match build_request(&uri, &user_agent, &referer) {
        Ok(request) => request,
        Err(_) => {
            stats.record_build_error();
            return;
        }
    };

    let response = match tokio::time::timeout(config.header_timeout, client.request(request)).await
    {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            stats.record_transport_error(classify_transport(&err));
            return;
        }
        Err(_) => {
            stats.record_transport_error(TransportKind::Timeout);
            return;
        }
    };

    let status = response.status();
    stats.record_status(status);
    stats.record_header_latency(worker_id, started.elapsed());

    let remain = config.timeout.saturating_sub(started.elapsed());
    if remain.is_zero() {
        stats.record_body_timeout();
        return;
    }

    let mut body = response.into_body();
    match tokio::time::timeout(remain, drain_body(&mut body, config.max_body)).await {
        Ok(Ok(Drain::Done)) => {
            stats.record_completed();
            stats.record_total_latency(started.elapsed());
        }
        Ok(Ok(Drain::Truncated)) => {
            stats.record_completed();
            stats.record_truncated();
            stats.record_total_latency(started.elapsed());
        }
        Ok(Err(_)) => stats.record_body_error(),
        Err(_) => stats.record_body_timeout(),
    }
}

fn build_request(
    uri: &str,
    user_agent: &str,
    referer: &str,
) -> Result<Request<Empty<Bytes>>, hyper::http::Error> {
    Request::get(uri)
        .header("user-agent", user_agent)
        .header("referer", referer)
        .body(Empty::<Bytes>::new())
}

enum Drain {
    Done,
    Truncated,
}

async fn drain_body(body: &mut Incoming, max_bytes: usize) -> Result<Drain, hyper::Error> {
    if max_bytes == 0 {
        return match body.frame().await {
            None => Ok(Drain::Done),
            Some(Ok(frame)) if frame.data_ref().is_some() => Ok(Drain::Truncated),
            Some(Ok(_)) => Ok(Drain::Done),
            Some(Err(err)) => Err(err),
        };
    }

    let mut read = 0usize;
    while let Some(next) = body.frame().await {
        let frame = next?;
        let Some(data) = frame.data_ref() else {
            continue;
        };
        read = read.saturating_add(data.len());
        if read >= max_bytes {
            return match body.frame().await {
                None => Ok(Drain::Done),
                Some(Ok(frame)) if frame.data_ref().is_some() => Ok(Drain::Truncated),
                Some(Ok(_)) => Ok(Drain::Done),
                Some(Err(err)) => Err(err),
            };
        }
    }
    Ok(Drain::Done)
}

fn pick<'a>(items: &'a [String], rng: &mut impl Rng) -> &'a str {
    &items[rng.gen_range(0..items.len())]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use url::Url;

    use crate::config::AppConfig;
    use crate::query::QueryPlan;
    use crate::stats::Stats;

    use super::*;

    #[tokio::test]
    async fn shared_client_reads_a_local_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                let n = socket.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .await
                .unwrap();
        });

        let config = AppConfig {
            target: Url::parse(&format!("http://{addr}/")).unwrap(),
            max_connections: 1,
            verbose: false,
            user_agents: Arc::<[String]>::from(vec!["hulk-rs".to_string()]),
            referers: Arc::<[String]>::from(vec!["https://example.com/".to_string()]),
            referers_exact: true,
            query: QueryPlan {
                fuzz: Vec::new(),
                cache_bust: false,
                default_length: 10,
            },
            header_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(5),
            max_body: 1024,
        };
        let stats = Stats::new();
        let mut rng = StdRng::seed_from_u64(1);
        send_one(&build_client(), &config, &stats, &mut rng, 0).await;

        let snap = stats.snapshot();
        assert_eq!(snap.completed, 1);
        assert_eq!(snap.status_2xx, 1);
        assert_eq!(snap.in_flight, 0);
        assert_eq!(snap.transport_errors, 0);
        assert_eq!(snap.body_errors, 0);
        assert_eq!(snap.body_truncated, 0);
        assert!(snap.header_count >= 1);
        assert!(snap.total_count >= 1);
    }
}
