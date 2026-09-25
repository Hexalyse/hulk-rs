# hulk-rs

HULK DoS tool ported to Rust. HULK stands for Http Unbearable Load King

This project was inspired by [hulk](https://github.com/grafov/hulk) which is a Go port of the original Python HULK tool with some additional features.    
I just decided to port it to Rust as an exercice to learn Rust.

As with the Go port which uses goroutines instead of threads, the idea is to use [tokio](https://github.com/tokio-rs/tokio) tasks which should give similar performance as goroutines.

## What does it do ?

HULK works by making each request unique so caches are less likely to serve a stored response. By default it appends a GET parameter with a random name and value to the provided URL. You can instead keep the query string from the target URL and **fuzz** selected parameter values in place (no duplicate keys). User-Agent and Referer headers are randomized on every request.

The goal is to "bypass" eventual caching mechanisms, leading to the request being directed to the backend every single time, which can lead to resource loads sometimes >100x greater on the machine than when serving a static cached version of the page. This allows even a single machine with a slow-ish internet connection to create a Denial of Service on big, badly configured servers.

Query editing re-serializes parameters with standard percent-encoding. That can change the original encoding (for example `+` vs `%20`), which matters for signed URLs. When the target query contains `+`, `%`, or a signature-like parameter, hulk-rs prints a warning before it starts.

Each request reads at most `--max-body` bytes of the response (default 1MB). A response longer than that is counted as truncated and that connection is not reused. `--header-timeout` (default 30s) bounds connecting and receiving headers. `--timeout` (default 60s) bounds the whole request. `--timeout` must be at least as long as `--header-timeout`.

Integer fuzz values can be limited to a range with `NAME:int:MIN:MAX` (for example `page:int:1:50`). Without a range, integers use `0` through `2147483647`. `--exact-referers` sends Referer values unchanged; otherwise a short random suffix is appended.

## Disclaimer

This tool is designed to be used as a stress testing utility to test the resilience of a server to such type of DoS attacks, and may lead to complete Denial of Service if used on a badly configured server/application. Use it carefully and responsibly.

## How to install

You can install this tool directly with [cargo](https://doc.rust-lang.org/cargo/). Just run `cargo install hulk-rs`.

## How to build from source

Just run `cargo build --release` in the root of the repository, and the built executable should be in `target/release/`.

## How to use

```
Usage: hulk-rs [OPTIONS] <TARGET>

Arguments:
  <TARGET>  Target URL, including query parameters whose values should be kept

Options:
  -m <MAX_CONNECTIONS>             Maximum concurrent connections [default: 1000]
  -v, --verbose                    Show individual HTTP status codes in the live report
  -u <FILE>                        User-Agent list (overrides the built-in lists)
  -p, --fuzz <SPEC>                Query parameter to fuzz (repeatable).
                                   NAME, NAME:int, NAME:int:MIN:MAX, NAME:string, NAME:string:LEN
      --fuzz-type <TYPE>           Default fuzz type: string or integer [default: string]
      --fuzz-length <LEN>          Default random string length [default: 10]
  -r <FILE>                        Referer list. A random suffix is appended unless --exact-referers
      --exact-referers             Use Referer values exactly as written
      --include-bots               Also include bot and search-engine user agents
      --bots-only                  Use only bot and search-engine user agents
      --header-timeout <DURATION>  Connect and response-header limit [default: 30s]
      --timeout <DURATION>         Whole-request limit, including the body [default: 60s]
      --max-body <SIZE>            Response body to read per request [default: 1MB]
  -h, --help                       Print help
  -V, --version                    Print version
```

Durations accept `ms`, `s`, `m`, and `h` (`500ms`, `30s`, `2m`). Body sizes accept plain bytes or a `K`/`M`/`G` suffix (`256KB`, `1MB`).

A live dashboard on stderr shows, in place, the recent request rate and the lifetime average, in-flight requests, status classes, and transport failures (including DNS and local resource errors such as port exhaustion). With `-v`, exact status codes are listed too. Latency to headers reports mean, min, p50, p95, p99, and max; time to complete reports mean, min, and max. Empty latency samples show as `n/a`. 2xx is green, 3xx yellow, 4xx/5xx and transport/body errors red. Press Ctrl+C for a graceful stop and a final report.

When `-u` is set, `--include-bots` and `--bots-only` are ignored.

Examples:

Most simple usage (classic HULK: append a random query parameter):    
`hulk-rs https://example.com/`

Keep existing query params, randomize only `page` as an integer (other params stay as given):    
`hulk-rs -p page:int "http://example.com/search?q=test&page=1&sort=name"`

Fuzz `page` inside `1..=50` and `q` as a 16-character string:    
`hulk-rs -p page:int:1:50 -p q:string:16 "http://example.com/search?q=test&page=1"`

100 concurrent connections, user agents from a file:    
`hulk-rs -m 100 -u /path/to/user_agents_file http://example.com/game.php?action=newgame`

Use a Referer file without appending a random suffix:    
`hulk-rs --exact-referers -r /path/to/referers.txt https://example.com/`

Include bot user agents as well as browsers:    
`hulk-rs --include-bots https://example.com/`

Use only search/bot user agents:    
`hulk-rs --bots-only https://example.com/`
