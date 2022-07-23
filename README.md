# hulk-rs

HULK DoS tool ported to Rust. HULK stands for Http Unbearable Load King

This project was inspired by [hulk](https://github.com/grafov/hulk) which is a Go port of the original Python HULK tool with some additional features.    
I just decided to port it to Rust as an exercice to learn Rust.

As with the Go port which uses goroutines instead of threads, the idea is to use [tokio](https://github.com/tokio-rs/tokio) tasks which should give similar performance as goroutines.

## What does it do ?

HULK works by appending a GET parameter with random name and value to the provided URL (hulk-rs also provide an option to choose a fixed name for this GET parameter). It also randomizes the User-Agent and the Referer HTTP headers.    
The goal is to "bypass" eventual caching mechanisms, leading to the request being directed to the backend every single time, which can lead to resource loads sometimes >100x greater on the machine than when serving a static cached version of the page. This allows even a single machine with a slow-ish internet connection to create a Denial of Service on big, badly configured servers.

## Disclaimer

This tool is designed to be used as a stress testing utility to test the resilience of a server to such type of DoS attacks, and may lead to complete Denial of Service if used on a badly configured server/application. Use it carefully and responsibly.

## How to build

Just run `cargo build --release` in the root of the repository, and the built executable should be in `target/release/`.

## How to use

```
USAGE:
    hulk-rs [OPTIONS] <TARGET>

ARGS:
    <TARGET>    Target URL (eg. http://example.com)

OPTIONS:
    -h, --help                   Print help information
    -m <MAX_CONNECTIONS>         Maximum number of concurrent connections to the target [default:
                                 1000]
    -p <PARAMETER_NAME>          Name of a GET parameter to add to the request (the value will be
                                 fuzzed, instead of fuzzing both the name of a GET parameter and its
                                 value)
    -r <REFERERS_FILE>           File containing a list of Referers to use
    -u <USER_AGENTS_FILE>        File containing a list of user agents to use
    -v, --verbose                verbose mode (display HTTP error codes)
```

Examples:

Most simple usage:    
`hulk-rs https://example.com/`

Target a specific GET parameter (the parameter will be APPENDED to the given target URL), with only 100 concurrent connections, with User-Agents loaded from a file:    
`hulk-rs -m 100 -p playername -u /path/to/user_agents_file http://example.com/game.php?action=newgame`    
(the generated URLs will look like `http://example.com/game.php?action=newgame&playername=<random_string>`

