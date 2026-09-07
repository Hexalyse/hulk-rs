use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hyper::header::HeaderValue;
use url::Url;

use crate::cli::CliArguments;
use crate::error::AppError;
use crate::query::{self, QueryPlan};
use crate::ua;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub target: Url,
    pub max_connections: usize,
    pub verbose: bool,
    pub user_agents: Arc<[String]>,
    pub referers: Arc<[String]>,
    pub query: QueryPlan,
}

impl AppConfig {
    pub fn from_cli(args: CliArguments) -> Result<Self, AppError> {
        if args.max_connections == 0 {
            return Err(AppError::InvalidMaxConnections);
        }

        query::validate_fuzz_length(args.fuzz_length)?;

        let target = parse_target(&args.target)?;
        let fuzz = query::parse_fuzz_specs(&args.fuzz, args.fuzz_type, args.fuzz_length)?;
        let cache_bust = fuzz.is_empty();

        let user_agents = if let Some(path) = &args.user_agents_file {
            load_lines(path, "user-agent")?
        } else {
            ua::builtin_user_agents(args.include_bots, args.bots_only)
        };
        if user_agents.is_empty() {
            return Err(AppError::EmptyList { what: "user-agent" });
        }
        validate_header_values("User-Agent", &user_agents)?;

        let referers = if let Some(path) = &args.referers_file {
            load_lines(path, "Referer")?
        } else {
            ua::builtin_referers()
        };
        if referers.is_empty() {
            return Err(AppError::EmptyList { what: "Referer" });
        }
        validate_header_values("Referer", &referers)?;

        Ok(Self {
            target,
            max_connections: args.max_connections,
            verbose: args.verbose,
            user_agents: user_agents.into(),
            referers: referers.into(),
            query: QueryPlan {
                fuzz,
                cache_bust,
                default_length: args.fuzz_length,
            },
        })
    }
}

fn parse_target(raw: &str) -> Result<Url, AppError> {
    let url = Url::parse(raw).map_err(|source| AppError::InvalidUrl {
        url: raw.to_string(),
        source,
    })?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(AppError::UnsupportedScheme {
                url: raw.to_string(),
                scheme: scheme.to_string(),
            });
        }
    }
    if url.host_str().is_none() {
        return Err(AppError::MissingHost {
            url: raw.to_string(),
        });
    }
    Ok(url)
}

fn load_lines(path: impl AsRef<Path>, what: &'static str) -> Result<Vec<String>, AppError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| AppError::Io {
        path: PathBuf::from(path),
        source,
    })?;
    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| AppError::Io {
            path: PathBuf::from(path),
            source,
        })?
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        return Err(AppError::EmptyFile {
            path: PathBuf::from(path),
            what,
        });
    }
    Ok(lines)
}

fn validate_header_values(what: &'static str, values: &[String]) -> Result<(), AppError> {
    for value in values {
        if HeaderValue::from_str(value).is_err() {
            return Err(AppError::InvalidHeaderValue {
                what,
                value: value.clone(),
            });
        }
    }
    Ok(())
}
