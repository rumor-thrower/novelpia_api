//! `/proc/user` and `/proc/viewer_board_comment` endpoint implementations.

use serde::Deserialize;
use serde_json::Value;

use crate::{
    client::Toggle,
    error::{Error, Result},
    models::{MemberBlockChk, MemberView, Novel},
    response::{parse_base, BaseResponse},
    Client,
};

// ---------------------------------------------------------------------------
// /proc/user modes
// ---------------------------------------------------------------------------

impl Client {
    /// `POST /proc/user` `mode=get_member2` — fetch raw member profile JSON.
    ///
    /// Returns the `result` value as raw JSON; callers deserialise fields they
    /// care about. Public, no auth required.
    pub async fn get_member2(&self, mem_no: u64) -> Result<Value> {
        self.user_post_raw("get_member2", mem_no).await
    }

    /// `POST /proc/user` `mode=get_member_view` — fetch member statistics.
    pub async fn get_member_view(&self, mem_no: u64) -> Result<MemberView> {
        let body = self.user_post_body("get_member_view", mem_no).await?;
        let r: BaseResponse<MemberView> = parse_base(&body)?;
        r.into_result()
    }

    /// `POST /proc/user` `mode=get_member_writer_novel` — novels written by a member.
    pub async fn get_member_writer_novel(&self, mem_no: u64) -> Result<Vec<Novel>> {
        let url = format!("{}/proc/user", self.base_url);
        let mem_no_s = mem_no.to_string();
        let resp = self
            .post_form(
                &url,
                &[
                    ("mode", "get_member_writer_novel"),
                    ("mem_no", &mem_no_s),
                    ("paging[rowCount]", "20"),
                    ("paging[curPage]", "1"),
                    ("paging[order]", "date"),
                    ("paging[sort][date]", "1"),
                ],
            )
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;

        #[derive(Deserialize)]
        struct Inner {
            novel: Vec<Novel>,
        }
        let r: BaseResponse<Inner> = parse_base(&body)?;
        Ok(r.into_result()?.novel)
    }

    /// `POST /proc/user` `mode=get_member_badge` — badge list for a member.
    pub async fn get_member_badge(&self, mem_no: u64) -> Result<Value> {
        self.user_post_raw("get_member_badge", mem_no).await
    }

    /// `POST /proc/user` `mode=get_member_emoticon` — owned emoticons for a member.
    pub async fn get_member_emoticon(&self, mem_no: u64) -> Result<Value> {
        self.user_post_raw("get_member_emoticon", mem_no).await
    }

    /// `POST /proc/user` `mode=get_member_stamp` — owned stamps for a member.
    pub async fn get_member_stamp(&self, mem_no: u64) -> Result<Value> {
        self.user_post_raw("get_member_stamp", mem_no).await
    }

    /// `POST /proc/user` `mode=get_member_keep_novel` — kept (bookmarked) novels.
    pub async fn get_member_keep_novel(&self, mem_no: u64) -> Result<Value> {
        self.user_post_raw("get_member_keep_novel", mem_no).await
    }

    /// `POST /proc/user` `mode=get_member_donation` — donation history.
    pub async fn get_member_donation(&self, mem_no: u64) -> Result<Value> {
        self.user_post_raw("get_member_donation", mem_no).await
    }

    /// `POST /proc/user` `mode=get_episode_cnt` — number of episodes read.
    pub async fn get_episode_cnt(&self, mem_no: u64) -> Result<Value> {
        self.user_post_raw("get_episode_cnt", mem_no).await
    }

    /// `POST /proc/user` `mode=get_stat_hall_of_fame` — hall-of-fame stats.
    ///
    /// `cate` is one of `"emoticon"`, `"donation"`, `"episode"`.
    pub async fn get_stat_hall_of_fame(&self, mem_no: u64, cate: &str) -> Result<Value> {
        let url = format!("{}/proc/user", self.base_url);
        let mem_no_s = mem_no.to_string();
        let resp = self
            .post_form(
                &url,
                &[
                    ("mode", "get_stat_hall_of_fame"),
                    ("mem_no", &mem_no_s),
                    ("cate", cate),
                ],
            )
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;
        let r: BaseResponse<Value> = parse_base(&body)?;
        r.into_result()
    }

    // -----------------------------------------------------------------------
    // Auth-required modes
    // -----------------------------------------------------------------------

    /// `POST /proc/user` `mode=get_member_favorite_novel` — favourited novels.
    ///
    /// Requires `LOGINKEY` authentication.
    pub async fn get_member_favorite_novel(&self) -> Result<Vec<Novel>> {
        self.require_auth()?;
        let url = format!("{}/proc/user", self.base_url);
        let resp = self
            .post_form(&url, &[("mode", "get_member_favorite_novel")])
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;

        #[derive(Deserialize)]
        struct Inner {
            novel: Vec<Novel>,
        }
        let r: BaseResponse<Inner> = parse_base(&body)?;
        Ok(r.into_result()?.novel)
    }

    /// `POST /proc/user` `mode=get_user_block_chk` — check if `mem_no` is blocked.
    ///
    /// Returns `Some(MemberBlockChk)` if the user is blocked, `None` if not.
    /// Requires `LOGINKEY` authentication.
    pub async fn get_user_block_chk(&self, mem_no: u64) -> Result<Option<MemberBlockChk>> {
        self.require_auth()?;
        let url = format!("{}/proc/user", self.base_url);
        let mem_no_s = mem_no.to_string();
        let resp = self
            .post_form(
                &url,
                &[("mode", "get_user_block_chk"), ("mem_no", &mem_no_s)],
            )
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;
        let r: BaseResponse<Value> = parse_base(&body)?;
        if r.status == 401 || r.status == 403 {
            return Err(Error::AuthRequired);
        }
        // `result` is null/false when not blocked, or an object when blocked.
        match r.result {
            None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(None),
            Some(v) => {
                let chk: MemberBlockChk =
                    serde_json::from_value(v).map_err(Error::Json)?;
                Ok(Some(chk))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    async fn user_post_body(&self, mode: &str, mem_no: u64) -> Result<String> {
        let url = format!("{}/proc/user", self.base_url);
        let mem_no_s = mem_no.to_string();
        let resp = self
            .post_form(&url, &[("mode", mode), ("mem_no", &mem_no_s)])
            .await?;
        resp.text().await.map_err(Error::Http)
    }

    async fn user_post_raw(&self, mode: &str, mem_no: u64) -> Result<Value> {
        let body = self.user_post_body(mode, mem_no).await?;
        let r: BaseResponse<Value> = parse_base(&body)?;
        r.into_result()
    }
}

// ---------------------------------------------------------------------------
// /proc/viewer_board_comment
// ---------------------------------------------------------------------------

impl Client {
    /// `POST /proc/viewer_board_comment` `mode=get_user_block` — get block list.
    ///
    /// Returns the list of blocked users' member numbers. Requires `LOGINKEY`.
    pub async fn get_user_block_list(&self, mem_no: u64) -> Result<Vec<u64>> {
        self.require_auth()?;
        let url = format!("{}/proc/viewer_board_comment", self.base_url);
        let mem_no_s = mem_no.to_string();
        let resp = self
            .post_form(
                &url,
                &[("mode", "get_user_block"), ("mem_no", &mem_no_s)],
            )
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;

        // The API returns `user_block` as an array of member-number strings,
        // e.g. `["68187", "512785", ...]`.
        #[derive(Deserialize)]
        struct Inner {
            user_block: Vec<String>,
        }
        let r: BaseResponse<Inner> = parse_base(&body)?;
        let inner = r.into_result()?;
        Ok(inner
            .user_block
            .into_iter()
            .filter_map(|s| s.parse().ok())
            .collect())
    }
}

// ---------------------------------------------------------------------------
// /proc/member_block
// ---------------------------------------------------------------------------

impl Client {
    /// `POST /proc/member_block` — toggle block status for a member.
    ///
    /// Requires `LOGINKEY` and a CSRF token.
    pub async fn toggle_member_block(&self, member_no: u64) -> Result<Toggle> {
        self.require_auth()?;
        let csrf = self.csrf.as_deref().unwrap_or("");
        let url = format!("{}/proc/member_block", self.base_url);
        let member_no_s = member_no.to_string();
        let resp = self
            .post_form(
                &url,
                &[("member_no", &member_no_s), ("csrf", csrf)],
            )
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;
        Toggle::parse(&body)
    }
}
