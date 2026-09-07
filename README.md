# hulk-rs

HULK DoS tool ported to Rust. HULK stands for Http Unbearable Load King

This project was inspired by [hulk](https://github.com/grafov/hulk) which is a Go port of the original Python HULK tool with some additional features.    
I just decided to port it to Rust as an exercice to learn Rust.

As with the Go port which uses goroutines instead of threads, the idea is to use [tokio](https://github.com/tokio-rs/tokio) tasks which should give similar performance as goroutines.

## What does it do ?

HULK works by making each request unique so caches are less likely to serve a stored response. By default it appends a GET parameter with a random name and value to the provided URL. You can instead keep the query string from the target URL and **fuzz** selected parameter values in place (no duplicate keys). User-Agent and Referer headers are randomized on every request.

The goal is to "bypass" eventual caching mechanisms, leading to the request being directed to the backend every single time, which can lead to resource loads sometimes >100x greater on the machine than when serving a static cached version of the page. This allows even a single machine with a slow-ish internet connection to create a Denial of Service on big, badly configured servers.

Query editing re-serializes parameters with standard percent-encoding. That can change the original encoding (for example `+` vs `%20`), which matters for signed URLs.

## Disclaimer

This tool is designed to be used as a stress testing utility to test the resilience of a server to such type of DoS attacks, and may lead to complete Denial of Service if used on a badly configured server/application. Use it carefully and responsibly.

## How to install

You can install this tool directly with [cargo](https://doc.rust-lang.org/cargo/). Just run `cargo install hulk-rs`.

## How to build from source

Just run `cargo build --release` in the root of the repository, and the built executable should be in `target/release/`.

## How to use

```
USAGE:
    hulk-rs [OPTIONS] <TARGET>

ARGS:
    <TARGET>    Target URL, including query parameters whose values should be kept

OPTIONS:
        --bots-only              Use only bot and search-engine user agents
    -h, --help                   Print help information
        --include-bots           Also include bot and search-engine user agents
    -m <MAX_CONNECTIONS>         Maximum number of concurrent connections [default: 1000]
    -p, --fuzz <SPEC>            Query parameter to fuzz (repeatable). Specs: NAME, NAME:int,
                                 NAME:string, NAME:string:LEN
        --fuzz-length <LEN>      Default random string length [default: 10]
        --fuzz-type <TYPE>       Default fuzz type: string or integer [default: string]
    -r <REFERERS_FILE>           File containing a list of Referers to use
    -u <USER_AGENTS_FILE>        File containing a list of user agents to use
    -v, --verbose                Display HTTP 4xx/5xx status codes
```

A live dashboard prints elapsed time, completed requests per second, in-flight count, status-class distribution, transport/body errors, and latency percentiles. Press Ctrl+C for a graceful stop and a final report.

When `-u` is set, `--include-bots` and `--bots-only` are ignored.

Examples:

Most simple usage (classic HULK: append a random query parameter):    
`hulk-rs https://example.com/`

Keep existing query params, randomize only `page` as an integer (other params stay as given):    
`hulk-rs -p page:int "http://example.com/search?q=test&page=1&sort=name"`

Fuzz `page` as an integer and `q` as a 16-character string:    
`hulk-rs -p page:int -p q:string:16 "http://example.com/search?q=test&page=1"`

100 concurrent connections, user agents from a file:    
`hulk-rs -m 100 -u /path/to/user_agents_file http://example.com/game.php?action=newgame`

Include bot user agents as well as browsers:    
`hulk-rs --include-bots https://example.com/`

Use only search/bot user agents:    
`hulk-rs --bots-only https://example.com/`
