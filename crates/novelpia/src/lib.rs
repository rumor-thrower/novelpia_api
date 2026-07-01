//! Unofficial Rust client for the Novelpia (novelpia.com) web-novel platform.
//!
//! This crate wraps Novelpia's reverse-engineered web endpoints (see the
//! `novelpia_openapi.yaml` spec in the repository root) behind typed models and
//! an async client. It is the foundation of a larger toolchain whose analysis
//! and visualization layers consume data exported by this crate.
//!
//! The API is unofficial and may change; parsers are written to fail soft with
//! typed errors rather than panic.

pub mod client;
pub mod endpoints;
pub mod error;
pub mod models;
pub mod response;

pub use client::{Client, ClientBuilder, Toggle};
pub use endpoints::episode::parse_episode_list_html;
pub use endpoints::misc::{EmoticonGroup, EmoticonGroupInfo, EmoticonItem};
pub use error::{Error, Result};

/// Base URL of the Novelpia Korean server.
pub const BASE_URL: &str = "https://novelpia.com";
