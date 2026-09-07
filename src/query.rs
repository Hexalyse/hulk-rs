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
}

#[derive(Debug, Clone)]
pub struct QueryPlan {
    /// Parameters whose values are randomized on every request.
    pub fuzz: Vec<FuzzParam>,
    /// When no named params are fuzzed, append a unique random name=value pair (classic HULK).
    pub cache_bust: bool,
    pub default_length: usize,
}

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

    match parts.len() {
        1 => Ok(FuzzParam {
            name: name.to_string(),
            kind: default_kind,
            length: default_length,
        }),
        2 => {
            let kind = parse_kind(parts[1]).map_err(invalid)?;
            Ok(FuzzParam {
                name: name.to_string(),
                kind,
                length: default_length,
            })
        }
        3 => {
            let kind = parse_kind(parts[1]).map_err(invalid)?;
            let length: usize = parts[2].trim().parse().map_err(|_| {
                invalid(format!(
                    "invalid length '{}' (expected a positive integer)",
                    parts[2]
                ))
            })?;
            if kind == FuzzKind::Integer {
                return Err(invalid(
                    "length is only valid with string fuzzing (use NAME:string:LEN)".to_string(),
                ));
            }
            validate_fuzz_length(length)?;
            Ok(FuzzParam {
                name: name.to_string(),
                kind,
                length,
            })
        }
        _ => Err(invalid(
            "too many ':' segments (expected NAME, NAME:int, NAME:string, or NAME:string:LEN)"
                .to_string(),
        )),
    }
}

fn parse_kind(raw: &str) -> Result<FuzzKind, String> {
    FuzzKind::from_str(raw.trim())
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

fn fuzz_value(param: &FuzzParam, rng: &mut impl Rng) -> String {
    match param.kind {
        FuzzKind::String => random_string(param.length, rng),
        FuzzKind::Integer => rng.gen_range(0..=i32::MAX).to_string(),
    }
}

pub fn random_string(n: usize, rng: &mut impl Rng) -> String {
    use rand::distributions::{Alphanumeric, DistString};
    Alphanumeric.sample_string(rng, n)
}
