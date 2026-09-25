use std::collections::HashSet;
use std::str::FromStr;

use rand::Rng;
use url::Url;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuzzKind {
    String,
    Integer,
}

impl FromStr for FuzzKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "string" | "str" => Ok(Self::String),
            "integer" | "int" => Ok(Self::Integer),
            _ => Err(format!(
                "invalid fuzz type '{s}' (expected string or integer)"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FuzzParam {
    pub name: String,
    pub kind: FuzzKind,
    pub length: usize,
    pub int_min: i64,
    pub int_max: i64,
}

#[derive(Debug, Clone)]
pub struct QueryPlan {
    /// Parameters whose values are randomized on every request.
    pub fuzz: Vec<FuzzParam>,
    /// When no named params are fuzzed, append a unique random name=value pair (classic HULK).
    pub cache_bust: bool,
    pub default_length: usize,
}

/// Inclusive range used when an integer spec does not set MIN:MAX.
pub const DEFAULT_INT_MIN: i64 = 0;
pub const DEFAULT_INT_MAX: i64 = i32::MAX as i64;

pub fn parse_fuzz_specs(
    specs: &[String],
    default_kind: FuzzKind,
    default_length: usize,
) -> Result<Vec<FuzzParam>, AppError> {
    let mut parsed = Vec::with_capacity(specs.len());
    let mut seen = HashSet::new();

    for spec in specs {
        let param = parse_fuzz_spec(spec, default_kind, default_length)?;
        if !seen.insert(param.name.clone()) {
            return Err(AppError::InvalidFuzzSpec {
                spec: spec.clone(),
                reason: format!(
                    "query parameter '{}' is specified more than once",
                    param.name
                ),
            });
        }
        parsed.push(param);
    }

    Ok(parsed)
}

fn parse_fuzz_spec(
    spec: &str,
    default_kind: FuzzKind,
    default_length: usize,
) -> Result<FuzzParam, AppError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(AppError::InvalidFuzzSpec {
            spec: spec.to_string(),
            reason: "name is empty".to_string(),
        });
    }

    let parts: Vec<&str> = spec.split(':').collect();
    let name = parts[0].trim();
    if name.is_empty() {
        return Err(AppError::InvalidFuzzSpec {
            spec: spec.to_string(),
            reason: "name is empty".to_string(),
        });
    }

    let invalid = |reason: String| AppError::InvalidFuzzSpec {
        spec: spec.to_string(),
        reason,
    };

    let (kind, length, int_min, int_max) = match parts.len() {
        1 => (
            default_kind,
            default_length,
            DEFAULT_INT_MIN,
            DEFAULT_INT_MAX,
        ),
        2 => {
            let kind = parse_kind(parts[1]).map_err(invalid)?;
            (kind, default_length, DEFAULT_INT_MIN, DEFAULT_INT_MAX)
        }
        3 => {
            let kind = parse_kind(parts[1]).map_err(invalid)?;
            if kind == FuzzKind::Integer {
                return Err(invalid(
                    "integer range needs MIN and MAX (use NAME:int:MIN:MAX)".to_string(),
                ));
            }
            let length = parse_length(parts[2]).map_err(invalid)?;
            check_string_length(length).map_err(invalid)?;
            (kind, length, DEFAULT_INT_MIN, DEFAULT_INT_MAX)
        }
        4 => {
            let kind = parse_kind(parts[1]).map_err(invalid)?;
            if kind != FuzzKind::Integer {
                return Err(invalid(
                    "length is a single number (use NAME:string:LEN)".to_string(),
                ));
            }
            let int_min = parse_i64(parts[2]).map_err(invalid)?;
            let int_max = parse_i64(parts[3]).map_err(invalid)?;
            check_int_range(int_min, int_max).map_err(invalid)?;
            (kind, default_length, int_min, int_max)
        }
        _ => {
            return Err(invalid(
                "too many ':' segments (expected NAME, NAME:int, NAME:int:MIN:MAX, NAME:string, or NAME:string:LEN)"
                    .to_string(),
            ));
        }
    };

    Ok(FuzzParam {
        name: name.to_string(),
        kind,
        length,
        int_min,
        int_max,
    })
}

fn parse_kind(raw: &str) -> Result<FuzzKind, String> {
    FuzzKind::from_str(raw.trim())
}

fn parse_length(raw: &str) -> Result<usize, String> {
    raw.trim()
        .parse()
        .map_err(|_| format!("invalid length '{raw}' (expected a positive integer)"))
}

fn parse_i64(raw: &str) -> Result<i64, String> {
    raw.trim()
        .parse()
        .map_err(|_| format!("invalid integer '{raw}'"))
}

fn check_string_length(length: usize) -> Result<(), String> {
    if (1..=4096).contains(&length) {
        Ok(())
    } else {
        Err(format!("invalid length {length} (expected 1 through 4096)"))
    }
}

fn check_int_range(min: i64, max: i64) -> Result<(), String> {
    if min > max {
        return Err(format!("integer range {min}..={max} is inverted"));
    }
    let count = (max as i128) - (min as i128) + 1;
    if count > u64::MAX as i128 {
        return Err("integer range is too wide".to_string());
    }
    Ok(())
}

pub fn validate_fuzz_length(length: usize) -> Result<(), AppError> {
    if (1..=4096).contains(&length) {
        Ok(())
    } else {
        Err(AppError::InvalidFuzzLength(length))
    }
}

/// Rebuild the request URL: keep non-fuzzed query values, replace fuzzed names, preserve fragment
/// separately from the request target. Query pairs are re-serialized with standard
/// percent-encoding, which may normalize the original encoding.
pub fn build_request_url(template: &Url, plan: &QueryPlan, rng: &mut impl Rng) -> Url {
    let mut url = template.clone();
    let existing: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    url.set_query(None);
    {
        let mut writer = url.query_pairs_mut();
        writer.clear();

        let mut replaced = HashSet::new();
        for (name, value) in &existing {
            if let Some(param) = plan.fuzz.iter().find(|p| p.name == *name) {
                writer.append_pair(name, &fuzz_value(param, rng));
                replaced.insert(name.as_str());
            } else {
                writer.append_pair(name, value);
            }
        }

        for param in &plan.fuzz {
            if !replaced.contains(param.name.as_str()) {
                writer.append_pair(&param.name, &fuzz_value(param, rng));
            }
        }

        if plan.cache_bust {
            writer.append_pair(
                &random_string(10, rng),
                &random_string(plan.default_length, rng),
            );
        }
    }

    url
}

/// Request target without fragment. Hyper request URIs must not include fragments.
pub fn request_target(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.to_string()
}

/// Warn when re-encoding is likely to change a signed or unusually encoded query.
pub fn reserialize_warning(url: &Url) -> Option<&'static str> {
    let query = url.query()?;
    if query.is_empty() {
        return None;
    }
    let raw_sensitive = query.contains('+') || query.contains('%');
    let signed = url.query_pairs().any(|(name, _)| is_signature_name(&name));
    if raw_sensitive || signed {
        Some(
            "query string will be re-encoded; '+' / percent-encoding may change, and signed URLs can fail",
        )
    } else {
        None
    }
}

fn is_signature_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("x-amz-")
        || ["sig", "signature", "hmac", "expires", "token"]
            .iter()
            .any(|hint| name.contains(hint))
}

pub fn format_referer(base: &str, exact: bool, rng: &mut impl Rng) -> String {
    if exact {
        base.to_string()
    } else {
        let n = rng.gen_range(5..10);
        format!("{base}{}", random_string(n, rng))
    }
}

fn fuzz_value(param: &FuzzParam, rng: &mut impl Rng) -> String {
    match param.kind {
        FuzzKind::String => random_string(param.length, rng),
        FuzzKind::Integer => rng.gen_range(param.int_min..=param.int_max).to_string(),
    }
}

pub fn random_string(n: usize, rng: &mut impl Rng) -> String {
    use rand::distributions::{Alphanumeric, DistString};
    Alphanumeric.sample_string(rng, n)
}

#[cfg(test)]
mod tests {
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    use super::*;

    fn plan(fuzz: Vec<FuzzParam>, cache_bust: bool) -> QueryPlan {
        QueryPlan {
            fuzz,
            cache_bust,
            default_length: 8,
        }
    }

    #[test]
    fn parses_integer_range_and_string_length() {
        let ranged = parse_fuzz_specs(&["page:int:1:50".into()], FuzzKind::String, 10).unwrap();
        assert_eq!(ranged[0].kind, FuzzKind::Integer);
        assert_eq!((ranged[0].int_min, ranged[0].int_max), (1, 50));

        let open = parse_fuzz_specs(&["page:int".into()], FuzzKind::String, 10).unwrap();
        assert_eq!(
            (open[0].int_min, open[0].int_max),
            (DEFAULT_INT_MIN, DEFAULT_INT_MAX)
        );

        let text = parse_fuzz_specs(&["q:string:16".into()], FuzzKind::String, 10).unwrap();
        assert_eq!(text[0].length, 16);
        assert_eq!(text[0].kind, FuzzKind::String);
    }

    #[test]
    fn rejects_bad_fuzz_specs() {
        assert!(parse_fuzz_specs(&["page:int:1".into()], FuzzKind::String, 10).is_err());
        assert!(parse_fuzz_specs(&["page:int:5:1".into()], FuzzKind::String, 10).is_err());
        assert!(parse_fuzz_specs(&["q:string:10:2".into()], FuzzKind::String, 10).is_err());
        assert!(parse_fuzz_specs(
            &["page:int".into(), "page:string".into()],
            FuzzKind::String,
            10
        )
        .is_err());
    }

    #[test]
    fn replaces_fuzzed_params_and_keeps_the_rest() {
        let template = Url::parse("http://example.com/search?q=test&page=1&sort=name#top").unwrap();
        let fuzz = parse_fuzz_specs(
            &["page:int:1:50".into(), "missing:string:4".into()],
            FuzzKind::String,
            10,
        )
        .unwrap();
        let mut rng = StdRng::seed_from_u64(7);
        let url = build_request_url(&template, &plan(fuzz, false), &mut rng);

        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(pairs[0].0, "q");
        assert_eq!(pairs[0].1, "test");
        assert_eq!(pairs[1].0, "page");
        let page: i64 = pairs[1].1.parse().unwrap();
        assert!((1..=50).contains(&page));
        assert_eq!(pairs[2].0, "sort");
        assert_eq!(pairs[2].1, "name");
        assert_eq!(pairs[3].0, "missing");
        assert_eq!(pairs[3].1.len(), 4);
        assert_eq!(url.fragment(), Some("top"));
        assert!(!request_target(&url).contains('#'));
    }

    #[test]
    fn cache_bust_appends_one_pair() {
        let template = Url::parse("http://example.com/").unwrap();
        let mut rng = StdRng::seed_from_u64(3);
        let url = build_request_url(&template, &plan(Vec::new(), true), &mut rng);
        assert_eq!(url.query_pairs().count(), 1);
    }

    #[test]
    fn warns_when_reencoding_can_change_the_query() {
        assert!(reserialize_warning(&Url::parse("http://example.com/?q=test").unwrap()).is_none());
        assert!(reserialize_warning(&Url::parse("http://example.com/").unwrap()).is_none());
        assert!(reserialize_warning(&Url::parse("http://example.com/?q=a+b").unwrap()).is_some());
        assert!(reserialize_warning(&Url::parse("http://example.com/?q=a%20b").unwrap()).is_some());
        assert!(reserialize_warning(&Url::parse("http://example.com/?sig=abc").unwrap()).is_some());
    }

    #[test]
    fn exact_referer_keeps_the_value() {
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(
            format_referer("https://example.com/a", true, &mut rng),
            "https://example.com/a"
        );
        let suffixed = format_referer("https://example.com/?q=", false, &mut rng);
        assert!(suffixed.starts_with("https://example.com/?q="));
        assert!(suffixed.len() > "https://example.com/?q=".len());
    }
}
