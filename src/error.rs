use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum AppError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    InvalidUrl {
        url: String,
        source: url::ParseError,
    },
    UnsupportedScheme {
        url: String,
        scheme: String,
    },
    MissingHost {
        url: String,
    },
    InvalidHeaderValue {
        what: &'static str,
        value: String,
    },
    EmptyList {
        what: &'static str,
    },
    EmptyFile {
        path: PathBuf,
        what: &'static str,
    },
    InvalidFuzzSpec {
        spec: String,
        reason: String,
    },
    InvalidFuzzLength(usize),
    InvalidMaxConnections,
    InvalidTimeout,
    WorkerPanics(usize),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::InvalidUrl { url, source } => {
                write!(f, "invalid target URL '{url}': {source}")
            }
            Self::UnsupportedScheme { url, scheme } => {
                write!(
                    f,
                    "unsupported URL scheme '{scheme}' in '{url}' (expected http or https)"
                )
            }
            Self::MissingHost { url } => {
                write!(f, "target URL '{url}' is missing a host")
            }
            Self::InvalidHeaderValue { what, value } => {
                write!(f, "invalid {what} header value: {value:?}")
            }
            Self::EmptyList { what } => {
                write!(f, "{what} list is empty")
            }
            Self::EmptyFile { path, what } => {
                write!(
                    f,
                    "{what} file {} did not contain any non-empty lines",
                    path.display()
                )
            }
            Self::InvalidFuzzSpec { spec, reason } => {
                write!(f, "invalid --fuzz spec '{spec}': {reason}")
            }
            Self::InvalidFuzzLength(len) => {
                write!(f, "invalid --fuzz-length {len} (expected 1 through 4096)")
            }
            Self::InvalidMaxConnections => {
                write!(f, "maximum connections must be at least 1")
            }
            Self::InvalidTimeout => {
                write!(
                    f,
                    "--timeout must be greater than or equal to --header-timeout"
                )
            }
            Self::WorkerPanics(n) => {
                write!(f, "{n} worker task(s) panicked")
            }
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidUrl { source, .. } => Some(source),
            _ => None,
        }
    }
}
