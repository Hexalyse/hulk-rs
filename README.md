# hulk-rs

HULK DoS tool ported to Rust.

This project was inspired by [hulk](https://github.com/grafov/hulk) which is a Go port of the original Python HULK tool, with some additional features. I just decided to port it to Rust as an exercice to learn Rust.

As with the Go port which uses goroutines instead of threads, the idea is to use [tokio](https://github.com/tokio-rs/tokio) which should give similar performance as goroutines.

## Disclaimer

This tool is designed to be used as a stress testing utility, and may lead to complete Denial of Service if used on a badly configured server/application. Use it carefully and responsibly.
