//! Miscellaneous endpoints: alarm, member_plus, emoticon_proc, emoticon_openstore.

use serde::Deserialize;
use serde_json::Value;

use crate::{
    error::{Error, Result},
    response::{parse_base, BaseResponse},
    Client,
};

// ---------------------------------------------------------------------------
// /proc/alarm
// ---------------------------------------------------------------------------

impl Client {
    /// `POST /proc/alarm` `mode=getAlarmCnt` — alarm/notification count.
    ///
    /// Returns the count as `u64`. Returns 0 when unauthenticated (server
    /// returns status 200 with cnt=0).
    pub async fn get_alarm_cnt(&self) -> Result<u64> {
        let url = format!("{}/proc/alarm", self.base_url);
        let resp = self
            .post_form(&url, &[("mode", "getAlarmCnt")])
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;

        #[derive(Deserialize)]
        struct AlarmResult {
            cnt: u64,
        }
        let r: BaseResponse<AlarmResult> = parse_base(&body)?;
        // Non-2xx body status → auth or error; let into_result handle it.
        Ok(r.into_result().map(|a| a.cnt).unwrap_or(0))
    }
}

// ---------------------------------------------------------------------------
// /proc/member_plus
// ---------------------------------------------------------------------------

impl Client {
    /// `GET /proc/member_plus?cmd=event_list` — PLUS membership event list.
    pub async fn get_member_plus_event_list(&self) -> Result<Value> {
        let url = format!("{}/proc/member_plus?cmd=event_list", self.base_url);
        let resp = self.get(&url).await?;
        let body = resp.text().await.map_err(Error::Http)?;
        serde_json::from_str(&body).map_err(Error::Json)
    }
}

// ---------------------------------------------------------------------------
// /proc/emoticon_proc
// ---------------------------------------------------------------------------

/// An emoticon group entry from `get_user_emoticon_group`.
#[derive(Debug, Clone, Deserialize)]
pub struct EmoticonGroup {
    pub emoticon_group: u64,
    #[serde(default)]
    pub emoticon_group_name: Option<String>,
    /// Present when `target=sort`; controls display order / visibility.
    #[serde(default)]
    pub is_hide: Option<u8>,
}

/// An emoticon item entry from `get_user_emoticon`.
#[derive(Debug, Clone, Deserialize)]
pub struct EmoticonItem {
    pub emoticon_no: u64,
    #[serde(default)]
    pub emoticon_group: Option<u64>,
    #[serde(default)]
    pub emoticon_img: Option<String>,
}

impl Client {
    /// `GET /proc/emoticon_proc?mode=get_user_emoticon_group` — emoticon groups owned by the user.
    ///
    /// When unauthenticated, the server returns status 200 with an empty `data` array.
    /// Pass `target_sort = true` to include the `target=sort` variant (adds `is_hide` flag).
    pub async fn get_user_emoticon_group(
        &self,
        target_sort: bool,
    ) -> Result<Vec<EmoticonGroup>> {
        let url = if target_sort {
            format!(
                "{}/proc/emoticon_proc?mode=get_user_emoticon_group&target=sort",
                self.base_url
            )
        } else {
            format!(
                "{}/proc/emoticon_proc?mode=get_user_emoticon_group",
                self.base_url
            )
        };
        let resp = self.get(&url).await?;
        let body = resp.text().await.map_err(Error::Http)?;

        #[derive(Deserialize)]
        struct Resp {
            #[serde(deserialize_with = "crate::response::de_status_pub")]
            status: u16,
            #[serde(default)]
            data: Vec<EmoticonGroup>,
        }
        let r: Resp = serde_json::from_str(&body).map_err(Error::Json)?;
        if r.status == 401 || r.status == 403 {
            return Err(Error::AuthRequired);
        }
        Ok(r.data)
    }

    /// `GET /proc/emoticon_proc?mode=get_user_emoticon&emoticon_group_no=<G>` — emoticons in a group.
    ///
    /// Requires `LOGINKEY`; server returns body `{"status":401,...}` when unauthenticated.
    pub async fn get_user_emoticon(&self, emoticon_group_no: u64) -> Result<Vec<EmoticonItem>> {
        let url = format!(
            "{}/proc/emoticon_proc?mode=get_user_emoticon&emoticon_group_no={}",
            self.base_url, emoticon_group_no
        );
        let resp = self.get(&url).await?;
        let body = resp.text().await.map_err(Error::Http)?;

        #[derive(Deserialize)]
        struct Resp {
            #[serde(deserialize_with = "crate::response::de_status_pub")]
            status: u16,
            #[serde(default)]
            data: Vec<EmoticonItem>,
        }
        let r: Resp = serde_json::from_str(&body).map_err(Error::Json)?;
        if r.status == 401 || r.status == 403 {
            return Err(Error::AuthRequired);
        }
        Ok(r.data)
    }
}

// ---------------------------------------------------------------------------
// /proc/emoticon_openstore
// ---------------------------------------------------------------------------

/// Result item from `getImgtoGrpEmt`.
#[derive(Debug, Clone, Deserialize)]
pub struct EmoticonGroupInfo {
    #[serde(default)]
    pub emoticon_group: Option<u64>,
    #[serde(default)]
    pub emoticon_group_name: Option<String>,
    #[serde(default)]
    pub stamp_name: Option<String>,
}

impl Client {
    /// `POST /proc/emoticon_openstore` `mode=getImgtoGrpEmt` — find emoticon group by image URL.
    ///
    /// Server returns a pipe-prefixed JSON array: `OK|[{...},...]`.
    pub async fn get_emoticon_group_by_img(
        &self,
        img_url: &str,
    ) -> Result<Vec<EmoticonGroupInfo>> {
        let url = format!("{}/proc/emoticon_openstore", self.base_url);
        let resp = self
            .post_form(&url, &[("mode", "getImgtoGrpEmt"), ("img", img_url)])
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;
        parse_ok_pipe_json(&body)
    }

    /// `GET /proc/emoticon_openstore?mode=getWriterEmoticon&novel_no=<N>` — emoticons for a novel's author.
    pub async fn get_writer_emoticon(&self, novel_no: u64) -> Result<Value> {
        let url = format!(
            "{}/proc/emoticon_openstore?mode=getWriterEmoticon&novel_no={}",
            self.base_url, novel_no
        );
        let resp = self.get(&url).await?;
        let body = resp.text().await.map_err(Error::Http)?;
        let r: BaseResponse<Value> = parse_base(&body)?;
        // Return the whole extra map as a Value since fields vary.
        Ok(Value::Object(
            r.extra
                .into_iter()
                .map(|(k, v)| (k, v))
                .collect(),
        ))
    }
}

// ---------------------------------------------------------------------------
// `OK|<json>` pipe response parser
// ---------------------------------------------------------------------------

fn parse_ok_pipe_json<T: serde::de::DeserializeOwned>(body: &str) -> Result<T> {
    let body = body.trim();
    let json_part = body
        .strip_prefix("OK|")
        .ok_or_else(|| Error::parse(format!("expected OK| prefix, got: {:?}", &body[..body.len().min(80)])))?;
    serde_json::from_str(json_part).map_err(Error::Json)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ok_pipe_json_emoticon_group() {
        let body = r#"OK|[{"emoticon_group":1700,"emoticon_group_name":"댓글창의 천재 마법사"}]"#;
        let items: Vec<EmoticonGroupInfo> = parse_ok_pipe_json(body).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].emoticon_group, Some(1700));
        assert_eq!(
            items[0].emoticon_group_name.as_deref(),
            Some("댓글창의 천재 마법사")
        );
    }

    #[test]
    fn parse_ok_pipe_json_missing_prefix_errors() {
        let body = r#"[{"emoticon_group":1}]"#;
        let err = parse_ok_pipe_json::<Vec<EmoticonGroupInfo>>(body).unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
    }
}
