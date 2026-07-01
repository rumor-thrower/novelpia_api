//! Async HTTP client — retry, User-Agent rotation, LOGINKEY cookie.

use std::time::Duration;

use rand::Rng;
use reqwest::{header, Response};

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// User-Agent pool — mirrors the Chrome/Edge/Firefox rotation used in PNWC.
// ---------------------------------------------------------------------------

const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:125.0) Gecko/20100101 Firefox/125.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:124.0) Gecko/20100101 Firefox/124.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Edg/124.0.0.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_4_1) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4.1 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
];

fn random_ua() -> &'static str {
    let idx = rand::thread_rng().gen_range(0..USER_AGENTS.len());
    USER_AGENTS[idx]
}

// ---------------------------------------------------------------------------
// Pipe-delimited toggle response.
// Novelpia returns e.g. `on|42||`, `off|42||`, or `login|0||`.
// ---------------------------------------------------------------------------

/// Parsed result of Novelpia's pipe-delimited toggle/action responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toggle {
    /// Action was accepted; the u64 is the server-returned count/id field.
    On(u64),
    /// Action was reversed; the u64 is the server-returned count/id field.
    Off(u64),
    /// Server rejected the request because the session is not authenticated.
    Login,
}

impl Toggle {
    /// Parse a raw pipe-delimited response body into a [`Toggle`].
    pub fn parse(body: &str) -> Result<Self> {
        let body = body.trim();
        let parts: Vec<&str> = body.split('|').collect();
        if parts.is_empty() {
            return Err(Error::parse(format!("empty toggle response: {:?}", body)));
        }
        match parts[0] {
            "login" => Ok(Toggle::Login),
            "on" | "off" => {
                let n: u64 = parts
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                if parts[0] == "on" {
                    Ok(Toggle::On(n))
                } else {
                    Ok(Toggle::Off(n))
                }
            }
            other => Err(Error::parse(format!(
                "unknown toggle sentinel: {:?}",
                other
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Async client for the Novelpia web API.
///
/// Holds a shared [`reqwest::Client`] and optional authentication state. All
/// request helpers rotate the User-Agent, apply a randomised inter-request
/// delay, and retry on transient errors with exponential backoff — mirroring
/// the tenacity pattern used in the Python PNWC reference implementation.
#[derive(Clone, Debug)]
pub struct Client {
    http: reqwest::Client,
    pub(crate) base_url: String,
    /// `LOGINKEY=<hex>_<hex>` value (the full cookie value, not the header).
    pub(crate) login_key: Option<String>,
    /// CSRF token extracted from a page, required by some write endpoints.
    pub(crate) csrf: Option<String>,
    /// Minimum delay between requests in milliseconds.
    pub(crate) delay_min_ms: u64,
    /// Maximum delay between requests in milliseconds.
    pub(crate) delay_max_ms: u64,
    /// Maximum number of retry attempts (not counting the initial try).
    pub(crate) max_retries: u32,
}

impl Client {
    /// Create a client with default settings and no authentication.
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Start building a [`Client`].
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Return a reference to the configured base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Return `true` if a `LOGINKEY` has been configured.
    pub fn is_authenticated(&self) -> bool {
        self.login_key.is_some()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn build_cookie_header(&self) -> Option<String> {
        self.login_key
            .as_deref()
            .map(|k| format!("LOGINKEY={}", k))
    }

    fn random_delay_ms(&self) -> u64 {
        if self.delay_min_ms >= self.delay_max_ms {
            return self.delay_min_ms;
        }
        rand::thread_rng().gen_range(self.delay_min_ms..=self.delay_max_ms)
    }

    /// Returns true for errors that are worth retrying.
    fn is_retryable(err: &reqwest::Error) -> bool {
        err.is_timeout() || err.is_connect() || err.is_request()
    }

    /// Sleep for a random delay in [delay_min_ms, delay_max_ms].
    async fn sleep_delay(&self) {
        let ms = self.random_delay_ms();
        if ms > 0 {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
    }

    /// Exponential back-off sleep: `min(1000 * 2^attempt, 10_000)` ms.
    async fn sleep_backoff(attempt: u32) {
        let base_ms: u64 = 1000u64.saturating_mul(1u64.wrapping_shl(attempt));
        let ms = base_ms.min(10_000);
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    // -----------------------------------------------------------------------
    // Request primitives (retry loop)
    // -----------------------------------------------------------------------

    /// Perform a GET request with retry/backoff/UA-rotation and optional delay.
    pub(crate) async fn get(&self, url: &str) -> Result<Response> {
        self.sleep_delay().await;
        let mut last_err: Option<Error> = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                Self::sleep_backoff(attempt - 1).await;
            }
            let mut rb = self
                .http
                .get(url)
                .header(header::USER_AGENT, random_ua())
                .header(header::REFERER, &self.base_url);
            if let Some(cookie) = self.build_cookie_header() {
                rb = rb.header(header::COOKIE, cookie);
            }
            match rb.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_server_error() {
                        last_err = Some(Error::Status {
                            status: status.as_u16(),
                            url: url.to_owned(),
                        });
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) if Self::is_retryable(&e) => {
                    last_err = Some(Error::Http(e));
                }
                Err(e) => return Err(Error::Http(e)),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            Error::parse(format!("GET {} failed after retries", url))
        }))
    }

    /// Perform a POST with form-encoded body, retry/backoff/UA-rotation and optional delay.
    pub(crate) async fn post_form(
        &self,
        url: &str,
        form: &[(&str, &str)],
    ) -> Result<Response> {
        self.sleep_delay().await;
        let mut last_err: Option<Error> = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                Self::sleep_backoff(attempt - 1).await;
            }
            let mut rb = self
                .http
                .post(url)
                .header(header::USER_AGENT, random_ua())
                .header(header::REFERER, &self.base_url)
                .form(form);
            if let Some(cookie) = self.build_cookie_header() {
                rb = rb.header(header::COOKIE, cookie);
            }
            match rb.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_server_error() {
                        last_err = Some(Error::Status {
                            status: status.as_u16(),
                            url: url.to_owned(),
                        });
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) if Self::is_retryable(&e) => {
                    last_err = Some(Error::Http(e));
                }
                Err(e) => return Err(Error::Http(e)),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            Error::parse(format!("POST {} failed after retries", url))
        }))
    }

    /// POST with an explicit per-request Referer (required by viewer_data).
    pub(crate) async fn post_form_with_referer(
        &self,
        url: &str,
        form: &[(&str, &str)],
        referer: &str,
    ) -> Result<Response> {
        self.sleep_delay().await;
        let mut last_err: Option<Error> = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                Self::sleep_backoff(attempt - 1).await;
            }
            let mut rb = self
                .http
                .post(url)
                .header(header::USER_AGENT, random_ua())
                .header(header::REFERER, referer)
                .form(form);
            if let Some(cookie) = self.build_cookie_header() {
                rb = rb.header(header::COOKIE, cookie);
            }
            match rb.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_server_error() {
                        last_err = Some(Error::Status {
                            status: status.as_u16(),
                            url: url.to_owned(),
                        });
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) if Self::is_retryable(&e) => {
                    last_err = Some(Error::Http(e));
                }
                Err(e) => return Err(Error::Http(e)),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            Error::parse(format!("POST {} failed after retries", url))
        }))
    }

    /// Assert that a `LOGINKEY` is configured; return `Err(AuthRequired)` otherwise.
    pub(crate) fn require_auth(&self) -> Result<&str> {
        self.login_key
            .as_deref()
            .ok_or(Error::AuthRequired)
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for [`Client`].
#[derive(Debug)]
pub struct ClientBuilder {
    base_url: String,
    login_key: Option<String>,
    csrf: Option<String>,
    delay_min_ms: u64,
    delay_max_ms: u64,
    max_retries: u32,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        ClientBuilder {
            base_url: crate::BASE_URL.to_owned(),
            login_key: None,
            csrf: None,
            delay_min_ms: 1_000,
            delay_max_ms: 3_000,
            max_retries: 4,
        }
    }
}

impl ClientBuilder {
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Set the `LOGINKEY` cookie value (the hex string after `LOGINKEY=`).
    pub fn login_key(mut self, key: impl Into<String>) -> Self {
        self.login_key = Some(key.into());
        self
    }

    /// Set the CSRF token for write endpoints.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Randomised inter-request delay range in milliseconds (default: 1000–3000).
    pub fn delay_ms(mut self, min: u64, max: u64) -> Self {
        self.delay_min_ms = min;
        self.delay_max_ms = max;
        self
    }

    /// Maximum retry attempts after the initial try (default: 4 → 5 total).
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    pub fn build(self) -> Client {
        let http = reqwest::Client::builder()
            .gzip(true)
            .build()
            .expect("failed to build reqwest::Client");
        Client {
            http,
            base_url: self.base_url,
            login_key: self.login_key,
            csrf: self.csrf,
            delay_min_ms: self.delay_min_ms,
            delay_max_ms: self.delay_max_ms,
            max_retries: self.max_retries,
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::Toggle;

    #[test]
    fn parse_on() {
        let t = Toggle::parse("on|42||").unwrap();
        assert_eq!(t, Toggle::On(42));
    }

    #[test]
    fn parse_off() {
        let t = Toggle::parse("off|7||").unwrap();
        assert_eq!(t, Toggle::Off(7));
    }

    #[test]
    fn parse_login() {
        let t = Toggle::parse("login|0||").unwrap();
        assert_eq!(t, Toggle::Login);
    }

    #[test]
    fn parse_trailing_newline() {
        let t = Toggle::parse("on|0||\n").unwrap();
        assert_eq!(t, Toggle::On(0));
    }

    #[test]
    fn parse_unknown_returns_err() {
        assert!(Toggle::parse("error|0||").is_err());
    }
}
