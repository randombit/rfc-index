use thiserror::Error;

/// Errors returned by the [`RfcIndex`](crate::RfcIndex) API.
#[derive(Debug, Error)]
pub enum Error {
    /// Underlying SQLite error (schema, query, or storage failure).
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// HTTP request failed (network error, TLS, non-success status, etc.).
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    /// XML parse failure (the rfc-index.xml or an RFC body).
    #[error("xml: {0}")]
    Xml(#[from] roxmltree::Error),
    /// Filesystem I/O failure (typically when creating the database directory).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Caller-supplied regular expression failed to compile.
    #[error("regex: {0}")]
    Regex(#[from] regex::Error),
    /// Resource not present (e.g. fetching the body of an unknown RFC).
    #[error("not found: {0}")]
    NotFound(String),
    /// Upstream payload didn't match the expected shape (corrupt or unexpected).
    #[error("malformed: {0}")]
    Malformed(String),
}

/// Convenience alias for `Result<T, Error>` used throughout the library.
pub type Result<T> = std::result::Result<T, Error>;
