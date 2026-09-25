use std::str::FromStr;
use std::time::Duration;

use clap::Parser;

use crate::query::FuzzKind;

#[derive(Parser, Debug)]
#[command(about = "HULK DoS tool", author, version)]
pub struct CliArguments {
    /// Maximum number of concurrent connections to the target
    #[arg(short, default_value_t = 1000)]
    pub max_connections: usize,

    /// Target URL, including query parameters whose values should be kept
    #[arg(value_name = "TARGET")]
    pub target: String,

    /// Show individual HTTP status codes in the live report
    #[arg(short, long)]
    pub verbose: bool,

    /// File containing a list of user agents to use (overrides built-in lists)
    #[arg(short, value_name = "FILE")]
    pub user_agents_file: Option<String>,

    /// Query parameter to fuzz. Repeatable. Existing values are replaced (no duplicates);
    /// missing names are added. Other query params keep the values from the URL.
    /// Specs: NAME, NAME:int, NAME:int:MIN:MAX, NAME:string, NAME:string:LEN
    #[arg(short = 'p', long = "fuzz", value_name = "SPEC")]
    pub fuzz: Vec<String>,

    /// Default fuzz value type when a --fuzz spec does not set one
    #[arg(
        long = "fuzz-type",
        default_value = "string",
        value_name = "TYPE",
        value_parser = parse_fuzz_kind
    )]
    pub fuzz_type: FuzzKind,

    /// Default random string length for --fuzz-type string
    #[arg(long = "fuzz-length", default_value_t = 10, value_name = "LEN")]
    pub fuzz_length: usize,

    /// File containing a list of Referers to use.
    /// A random suffix is appended unless --exact-referers is set.
    #[arg(short, value_name = "FILE")]
    pub referers_file: Option<String>,

    /// Use Referer values exactly as written, with no random suffix
    #[arg(long = "exact-referers")]
    pub referers_exact: bool,

    /// Also include bot and search-engine user agents alongside browser user agents
    #[arg(long = "include-bots", conflicts_with = "bots_only")]
    pub include_bots: bool,

    /// Use only bot and search-engine user agents instead of browser user agents
    #[arg(long = "bots-only", conflicts_with = "include_bots")]
    pub bots_only: bool,

    /// Time limit to connect and receive response headers
    #[arg(long = "header-timeout", default_value = "30s", value_parser = parse_duration)]
    pub header_timeout: Duration,

    /// Time limit for one request, including the body read
    #[arg(long = "timeout", default_value = "60s", value_parser = parse_duration)]
    pub timeout: Duration,

    /// Maximum response body to read per request
    #[arg(long = "max-body", default_value = "1MB", value_parser = parse_body_size, value_name = "SIZE")]
    pub max_body: usize,
}

pub(crate) fn parse_fuzz_kind(raw: &str) -> Result<FuzzKind, String> {
    FuzzKind::from_str(raw)
}

pub(crate) fn parse_duration(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    let split = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    if split == 0 {
        return Err(duration_error(raw));
    }
    let number: u64 = raw[..split].parse().map_err(|_| duration_error(raw))?;
    let suffix = raw[split..].trim().to_ascii_lowercase();
    let duration = match suffix.as_str() {
        "" | "s" => Duration::from_secs(number),
        "ms" => Duration::from_millis(number),
        "m" => Duration::from_secs(number.checked_mul(60).ok_or_else(|| duration_error(raw))?),
        "h" => Duration::from_secs(
            number
                .checked_mul(3600)
                .ok_or_else(|| duration_error(raw))?,
        ),
        _ => return Err(duration_error(raw)),
    };
    if duration.is_zero() {
        return Err("duration must be greater than zero".to_string());
    }
    Ok(duration)
}

fn duration_error(raw: &str) -> String {
    format!("invalid duration '{raw}' (expected a number with ms, s, m, or h)")
}

pub(crate) fn parse_body_size(raw: &str) -> Result<usize, String> {
    let raw = raw.trim();
    let split = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    if split == 0 {
        return Err(body_size_error(raw));
    }
    let number: u64 = raw[..split].parse().map_err(|_| body_size_error(raw))?;
    let suffix = raw[split..].trim().to_ascii_lowercase();
    let multiplier: u64 = match suffix.as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        "g" | "gb" => 1024 * 1024 * 1024,
        _ => return Err(body_size_error(raw)),
    };
    let bytes = number
        .checked_mul(multiplier)
        .ok_or_else(|| "body size overflows".to_string())?;
    usize::try_from(bytes).map_err(|_| "body size overflows".to_string())
}

fn body_size_error(raw: &str) -> String {
    format!("invalid body size '{raw}' (expected bytes or a K/M/G suffix)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("15").unwrap(), Duration::from_secs(15));
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn parses_body_sizes() {
        assert_eq!(parse_body_size("0").unwrap(), 0);
        assert_eq!(parse_body_size("1048576").unwrap(), 1024 * 1024);
        assert_eq!(parse_body_size("1MB").unwrap(), 1024 * 1024);
        assert_eq!(parse_body_size("256KB").unwrap(), 256 * 1024);
        assert!(parse_body_size("lots").is_err());
    }
}
