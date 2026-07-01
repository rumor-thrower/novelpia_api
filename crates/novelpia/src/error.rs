//! Error type for the Novelpia client.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the Novelpia client.
///
/// Parsers are written to fail soft: rather than panic on unexpected server
/// output, they return one of these typed variants so callers can decide how to
/// react (retry, skip, surface to the user).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The underlying HTTP transport failed (DNS, TLS, timeout, connection).
    #[error("http transport error: {0}")]
    Http(#[from] reqwest::Error),

    /// The server responded with a non-success HTTP status after retries.
    #[error("http status {status} for {url}")]
    Status {
        /// The HTTP status code.
        status: u16,
        /// The request URL.
        url: String,
    },

    /// The response body could not be decoded as the expected JSON shape.
    #[error("failed to decode json: {0}")]
    Json(#[from] serde_json::Error),

    /// The response body did not match an expected non-JSON shape
    /// (pipe-delimited toggles, HTML fragments, alert modals).
    #[error("unexpected response shape: {0}")]
    Parse(String),

    /// The endpoint requires authentication but no `LOGINKEY` was configured,
    /// or the server replied with the `login|0||` sentinel.
    #[error("authentication required (missing or rejected LOGINKEY)")]
    AuthRequired,

    /// The requested content is behind a paywall or otherwise not viewable
    /// (e.g. the viewer returned an alert modal instead of episode text).
    #[error("content not accessible: {0}")]
    NotAccessible(String),

    /// A required input was invalid (e.g. empty id list).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl Error {
    /// Construct a [`Error::Parse`] from anything displayable.
    pub(crate) fn parse(msg: impl fmt::Display) -> Self {
        Error::Parse(msg.to_string())
    }
}
