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

/// Truncate a string to at most `max_bytes` bytes without splitting a
/// multi-byte UTF-8 character.
///
/// Naive byte slicing (`&s[..s.len().min(max_bytes)]`) panics when the cut
/// point lands inside a multi-byte codepoint — a real hazard here because
/// Novelpia error bodies are Korean HTML. This backs off to the nearest
/// preceding char boundary so it is always safe.
pub(crate) fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::truncate_on_char_boundary;

    #[test]
    fn truncate_ascii_within_limit_is_identity() {
        assert_eq!(truncate_on_char_boundary("hello", 80), "hello");
    }

    #[test]
    fn truncate_ascii_over_limit_cuts_exactly() {
        assert_eq!(truncate_on_char_boundary("hello world", 5), "hello");
    }

    #[test]
    fn truncate_never_panics_on_multibyte_boundary() {
        // "가" is 3 bytes; a naive `&s[..80]` on this would panic mid-char.
        let s = "가".repeat(100); // 300 bytes
        let out = truncate_on_char_boundary(&s, 80);
        // 80 is not a multiple of 3, so it backs off to 78 (26 chars).
        assert_eq!(out.len(), 78);
        assert_eq!(out.chars().count(), 26);
        assert!(s.starts_with(out));
    }

    #[test]
    fn truncate_at_exact_char_boundary_keeps_full_chars() {
        let s = "가나다"; // 9 bytes
        assert_eq!(truncate_on_char_boundary(s, 6), "가나");
        assert_eq!(truncate_on_char_boundary(s, 9), "가나다");
    }

    #[test]
    fn truncate_zero_bytes_is_empty() {
        assert_eq!(truncate_on_char_boundary("가", 0), "");
    }
}
