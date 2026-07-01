//! Common response wrapper and helper parsers.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// BaseResponse<T>
// ---------------------------------------------------------------------------

/// JSON envelope returned by all `/proc/` endpoints.
///
/// `status` is either a quoted integer string or a bare integer depending on
/// the endpoint; the custom deserialiser handles both.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
pub struct BaseResponse<T = Value> {
    #[serde(deserialize_with = "de_status")]
    pub status: u16,
    #[serde(default)]
    pub errmsg: String,
    /// The payload field varies by endpoint — callers use the typed helpers
    /// ([`BaseResponse::into_result`]) rather than `result` directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    /// Additional fields forwarded as raw JSON (e.g. `list`, `data`).
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, Value>,
}

pub(crate) fn de_status_pub<'de, D>(d: D) -> std::result::Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de_status(d)
}

fn de_status<'de, D>(d: D) -> std::result::Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = u16;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("u16 or string-encoded u16")
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<u16, E> {
            u16::try_from(v).map_err(de::Error::custom)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<u16, E> {
            u16::try_from(v).map_err(de::Error::custom)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<u16, E> {
            v.parse().map_err(de::Error::custom)
        }
    }
    d.deserialize_any(V)
}

impl<T> BaseResponse<T> {
    /// Return `Ok(T)` if `result` is `Some` and the status is 2xx, otherwise
    /// surface a typed error.
    pub fn into_result(self) -> Result<T> {
        if self.status == 403 || self.status == 401 {
            return Err(Error::AuthRequired);
        }
        if !(200..300).contains(&(self.status as u32)) {
            return Err(Error::Status {
                status: self.status,
                url: String::new(),
            });
        }
        self.result
            .ok_or_else(|| Error::parse("missing `result` field in response"))
    }
}

// ---------------------------------------------------------------------------
// Typed response helpers
// ---------------------------------------------------------------------------

/// Deserialise a `BaseResponse<T>` from raw JSON bytes.
pub fn parse_base<T: DeserializeOwned>(body: &str) -> Result<BaseResponse<T>> {
    serde_json::from_str(body).map_err(Error::Json)
}

/// Deserialise `BaseResponse` then extract `result` as `Vec<T>` from the
/// `list` extra field (common pattern for list endpoints).
pub fn parse_list<T: DeserializeOwned>(body: &str, list_key: &str) -> Result<Vec<T>> {
    let raw: BaseResponse<Value> = parse_base(body)?;
    if raw.status == 403 || raw.status == 401 {
        return Err(Error::AuthRequired);
    }
    let list_val = raw
        .extra
        .get(list_key)
        .ok_or_else(|| Error::parse(format!("missing `{}` in response", list_key)))?;
    serde_json::from_value(list_val.clone()).map_err(Error::Json)
}

// ---------------------------------------------------------------------------
// Viewer text parser
// ---------------------------------------------------------------------------

/// Convert a `Vec<EpisodeTextLine>` JSON array body into plain text lines,
/// replacing `"&nbsp;"` with an empty string (blank paragraph marker).
pub fn parse_viewer_lines(body: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Line {
        text: String,
    }
    // The viewer response is an object `{"s": [{"text": ...}], "c": ...}`; the
    // paragraph lines live under `s`. Older responses were a bare array, so
    // accept both shapes.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Viewer {
        Wrapped { s: Vec<Line> },
        Bare(Vec<Line>),
    }
    let lines: Vec<Line> = serde_json::from_str::<Viewer>(body)
        .map(|v| match v {
            Viewer::Wrapped { s } => s,
            Viewer::Bare(s) => s,
        })
        .map_err(|_| {
            // Server returns an HTML alert modal on paywall / login failure.
            if body.trim_start().starts_with('<') {
                Error::NotAccessible(
                    "viewer returned HTML instead of JSON (paywall or auth)".into(),
                )
            } else {
                Error::parse(format!(
                    "viewer response is not JSON: {:?}",
                    crate::error::truncate_on_char_boundary(body, 120)
                ))
            }
        })?;
    Ok(lines
        .into_iter()
        .map(|l| {
            if l.text == "&nbsp;" {
                String::new()
            } else {
                l.text
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_result_ok_on_2xx_with_result() {
        let r: BaseResponse<u64> = BaseResponse {
            status: 200,
            errmsg: String::new(),
            result: Some(5),
            extra: Default::default(),
        };
        assert_eq!(r.into_result().unwrap(), 5);
    }

    #[test]
    fn into_result_err_auth_required_on_401_and_403() {
        for status in [401u16, 403] {
            let r: BaseResponse<Value> = BaseResponse {
                status,
                errmsg: String::new(),
                result: None,
                extra: Default::default(),
            };
            assert!(matches!(r.into_result().unwrap_err(), Error::AuthRequired));
        }
    }

    #[test]
    fn into_result_err_status_on_other_non_2xx() {
        let r: BaseResponse<Value> = BaseResponse {
            status: 500,
            errmsg: "boom".into(),
            result: None,
            extra: Default::default(),
        };
        assert!(matches!(
            r.into_result().unwrap_err(),
            Error::Status { status: 500, .. }
        ));
    }

    #[test]
    fn into_result_err_parse_on_missing_result_with_2xx() {
        let r: BaseResponse<Value> = BaseResponse {
            status: 200,
            errmsg: String::new(),
            result: None,
            extra: Default::default(),
        };
        assert!(matches!(r.into_result().unwrap_err(), Error::Parse(_)));
    }

    #[test]
    fn base_response_status_string() {
        let json = r#"{"status":"200","errmsg":""}"#;
        let r: BaseResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.status, 200);
    }

    #[test]
    fn base_response_status_int() {
        let json = r#"{"status":200,"errmsg":""}"#;
        let r: BaseResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.status, 200);
    }

    #[test]
    fn parse_viewer_lines_nbsp() {
        let json = r#"[{"text":"Hello"},{"text":"&nbsp;"},{"text":"World"}]"#;
        let lines = parse_viewer_lines(json).unwrap();
        assert_eq!(lines, vec!["Hello", "", "World"]);
    }

    #[test]
    fn parse_viewer_lines_wrapped_object() {
        let json = r#"{"s":[{"text":"Hello"},{"text":"&nbsp;"},{"text":"World"}],"c":1}"#;
        let lines = parse_viewer_lines(json).unwrap();
        assert_eq!(lines, vec!["Hello", "", "World"]);
    }

    #[test]
    fn parse_viewer_lines_html_returns_not_accessible() {
        let html = "<html><body><script>alert('로그인 후 이용하세요')</script></body></html>";
        let err = parse_viewer_lines(html).unwrap_err();
        assert!(matches!(err, Error::NotAccessible(_)));
    }

    #[test]
    fn parse_viewer_lines_long_korean_non_json_does_not_panic() {
        // Non-JSON body that does not start with `<` (so it takes the Parse
        // branch) and is longer than the 120-byte truncation limit. The
        // formatter must back off to a char boundary rather than panic.
        let body = "오류가 발생했습니다 ".repeat(20);
        let err = parse_viewer_lines(&body).unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
    }

    #[test]
    fn episode_view_count_int() {
        use crate::models::EpisodeViewCount;
        let item = EpisodeViewCount {
            episode_no: 7146,
            count_view: "1,057".into(),
        };
        assert_eq!(item.count_view_int(), Some(1057));
    }
}
