use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author = "Hexalyse", about = "HULK DoS tool")]
struct CliArguments {
    #[clap(short, long, default_value = "1024")]
    /// Maximum number of concurrent connections to the target
    max_connections: usize,
    /// Target URL (eg. http://example.com)
    target: String,
}

fn main() {
    let args = CliArguments::parse();
    println!("{:?}", args);
}
