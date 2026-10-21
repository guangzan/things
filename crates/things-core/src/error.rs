use std::path::PathBuf;

use thiserror::Error;

/// Stable, user-facing error categories.
///
/// The CLI uses these codes in JSON error responses and MCP returns them in
/// tool errors. They intentionally describe recovery, not implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    Ambiguous,
    InvalidInput,
    InvalidDate,
    Unsupported,
    Database,
    Permission,
    AuthRequired,
    AppUnavailable,
    VerificationFailed,
    Conflict,
    Internal,
}

impl ErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Ambiguous => "ambiguous",
            Self::InvalidInput => "invalid_input",
            Self::InvalidDate => "invalid_date",
            Self::Unsupported => "unsupported",
            Self::Database => "database_error",
            Self::Permission => "permission_denied",
            Self::AuthRequired => "auth_required",
            Self::AppUnavailable => "app_unavailable",
            Self::VerificationFailed => "verification_failed",
            Self::Conflict => "conflict",
            Self::Internal => "internal_error",
        }
    }
}

/// A resolution candidate returned with ambiguity errors.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Candidate {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: crate::models::ItemType,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<crate::models::ItemStatus>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Things 3 database not found")]
    DatabaseNotFound,

    #[error("multiple Things 3 databases found: {0:?}")]
    MultipleDatabases(Vec<PathBuf>),

    #[error("cannot read Things 3 database at {path}: {source}")]
    DatabaseRead {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("Things 3 database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("{0}")]
    NotFound(String),

    #[error("{message}")]
    Ambiguous {
        message: String,
        candidates: Vec<Candidate>,
        /// True when more matches existed than `candidates` carries.
        candidates_truncated: bool,
    },

    #[error("{0}")]
    InvalidInput(String),

    #[error("{0}")]
    InvalidDate(String),

    #[error("{0}")]
    Unsupported(String),

    #[error("permission denied reading Things 3 data: {0}")]
    Permission(String),

    #[error(
        "Things URL-scheme authentication token is required; enable Things URLs in Things Settings > General"
    )]
    AuthRequired,

    #[error("could not launch Things 3: {0}")]
    AppUnavailable(String),

    #[error("write was sent but could not be verified: {0}")]
    VerificationFailed(String),

    #[error("conflicting fields: {0}")]
    Conflict(String),

    #[error("invalid configuration at {path}: {source}")]
    Config {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Internal(String),
}

impl Error {
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            // A missing or ambiguous database is an environment problem, not a
            // missing item, so it reports as a database failure.
            Self::DatabaseNotFound
            | Self::MultipleDatabases(_)
            | Self::DatabaseRead { .. }
            | Self::Database(_) => ErrorCode::Database,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Ambiguous { .. } => ErrorCode::Ambiguous,
            Self::InvalidInput(_) => ErrorCode::InvalidInput,
            Self::InvalidDate(_) => ErrorCode::InvalidDate,
            Self::Unsupported(_) => ErrorCode::Unsupported,
            Self::Permission(_) => ErrorCode::Permission,
            Self::AuthRequired => ErrorCode::AuthRequired,
            Self::AppUnavailable(_) => ErrorCode::AppUnavailable,
            Self::VerificationFailed(_) => ErrorCode::VerificationFailed,
            Self::Conflict(_) => ErrorCode::Conflict,
            Self::Config { .. } | Self::Io(_) | Self::Internal(_) => ErrorCode::Internal,
        }
    }

    #[must_use]
    pub fn candidates(&self) -> Option<&[Candidate]> {
        match self {
            Self::Ambiguous { candidates, .. } => Some(candidates),
            _ => None,
        }
    }

    #[must_use]
    pub const fn candidates_truncated(&self) -> bool {
        match self {
            Self::Ambiguous {
                candidates_truncated,
                ..
            } => *candidates_truncated,
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
