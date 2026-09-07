use clap::Parser;

use crate::query::FuzzKind;

#[derive(Parser, Debug)]
#[clap(author = "Hexalyse", about = "HULK DoS tool")]
pub struct CliArguments {
    /// Maximum number of concurrent connections to the target
    #[clap(short, default_value = "1000")]
    pub max_connections: usize,

    /// Target URL, including query parameters whose values should be kept
    #[clap(value_name = "TARGET")]
    pub target: String,

    /// Verbose mode (display HTTP 4xx/5xx status codes)
    #[clap(short, long, takes_value = false)]
    pub verbose: bool,

    /// File containing a list of user agents to use (overrides built-in lists)
    #[clap(short, takes_value = true, required = false, value_name = "FILE")]
    pub user_agents_file: Option<String>,

    /// Query parameter to fuzz. Repeatable. Existing values are replaced (no duplicates);
    /// missing names are added. Other query params keep the values from the URL.
    /// Specs: NAME, NAME:int, NAME:string, NAME:string:LEN
    #[clap(
        short = 'p',
        long = "fuzz",
        multiple_occurrences = true,
        value_name = "SPEC"
    )]
    pub fuzz: Vec<String>,

    /// Default fuzz value type when a --fuzz spec does not set one
    #[clap(long = "fuzz-type", default_value = "string", value_name = "TYPE")]
    pub fuzz_type: FuzzKind,

    /// Default random string length for --fuzz-type string
    #[clap(long = "fuzz-length", default_value = "10", value_name = "LEN")]
    pub fuzz_length: usize,

    /// File containing a list of Referers to use (a random string will be appended to each Referer)
    #[clap(short, takes_value = true, required = false, value_name = "FILE")]
    pub referers_file: Option<String>,

    /// Also include bot and search-engine user agents alongside browser user agents
    #[clap(long = "include-bots", conflicts_with = "bots-only")]
    pub include_bots: bool,

    /// Use only bot and search-engine user agents instead of browser user agents
    #[clap(long = "bots-only", conflicts_with = "include-bots")]
    pub bots_only: bool,
}
