# hulk-rs

HULK DoS tool ported to Rust. HULK stands for Http Unbearable Load King

This project was inspired by [hulk](https://github.com/grafov/hulk) which is a Go port of the original Python HULK tool with some additional features.    
I just decided to port it to Rust as an exercice to learn Rust.

As with the Go port which uses goroutines instead of threads, the idea is to use [tokio](https://github.com/tokio-rs/tokio) tasks which should give similar performance as goroutines.

## Disclaimer

This tool is designed to be used as a stress testing utility, and may lead to complete Denial of Service if used on a badly configured server/application. Use it carefully and responsibly.

## How to build

Just run `cargo build --release` in the root of the repository, and the built executable should be in `target/release/`.

## TODO

- Add possibility to load a list of user agents from a file
- Add possibility to "fuzz" a specific GET parameters, instead of fuzzing the parameter key too