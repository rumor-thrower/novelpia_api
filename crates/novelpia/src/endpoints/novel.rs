//! `/proc/novel` and `/proc/novel_curation` endpoint implementations.

use serde::Deserialize;
use serde_json::Value;

use crate::{
    Client,
    client::Toggle,
    error::{Error, Result},
    models::{EpisodeViewCount, Novel, NovelReviewItem},
    response::{parse_base, parse_list},
};

// ---------------------------------------------------------------------------
// Novel review list
// ---------------------------------------------------------------------------

impl Client {
    /// `GET /proc/novel?cmd=get_novel_review_list&target_novel_no=<N>`
    ///
    /// Returns the list of reviews (작품평) for a novel. Public, no auth required.
    pub async fn get_novel_review_list(&self, novel_no: u64) -> Result<Vec<NovelReviewItem>> {
        let url = format!(
            "{}/proc/novel?cmd=get_novel_review_list&target_novel_no={}",
            self.base_url, novel_no
        );
        let resp = self.get(&url).await?;
        let body = resp.text().await.map_err(Error::Http)?;
        parse_list::<NovelReviewItem>(&body, "data")
    }

    // -----------------------------------------------------------------------
    // Episode view count (batch)
    // -----------------------------------------------------------------------

    /// Maximum number of episode numbers sent to `get_episode_count_view` per
    /// request. The server silently truncates responses beyond ~1000 rows, so
    /// larger `episode_nos` slices must be split across multiple requests.
    const EPISODE_VIEW_COUNT_BATCH_SIZE: usize = 100;

    /// `POST /proc/novel` `cmd=get_episode_count_view` (new variant) or
    /// `cmd=get_episode_cnt_view` (legacy variant).
    ///
    /// Returns per-episode view counts for `episode_nos`. Batches up to
    /// [`Self::EPISODE_VIEW_COUNT_BATCH_SIZE`] episode numbers per request,
    /// issuing multiple requests and concatenating the results as needed.
    pub async fn get_episode_view_counts(
        &self,
        novel_no: u64,
        episode_nos: &[u64],
        use_legacy_cmd: bool,
    ) -> Result<Vec<EpisodeViewCount>> {
        if episode_nos.is_empty() {
            return Err(Error::InvalidArgument(
                "episode_nos must not be empty".into(),
            ));
        }
        let mut results = Vec::with_capacity(episode_nos.len());
        for chunk in episode_nos.chunks(Self::EPISODE_VIEW_COUNT_BATCH_SIZE) {
            results.extend(
                self.get_episode_view_counts_batch(novel_no, chunk, use_legacy_cmd)
                    .await?,
            );
        }
        Ok(results)
    }

    /// Single-request implementation backing [`Self::get_episode_view_counts`].
    /// Callers should keep `episode_nos` at or below
    /// [`Self::EPISODE_VIEW_COUNT_BATCH_SIZE`] entries.
    async fn get_episode_view_counts_batch(
        &self,
        novel_no: u64,
        episode_nos: &[u64],
        use_legacy_cmd: bool,
    ) -> Result<Vec<EpisodeViewCount>> {
        let url = format!("{}/proc/novel", self.base_url);
        let cmd = if use_legacy_cmd {
            "get_episode_cnt_view"
        } else {
            "get_episode_count_view"
        };
        let novel_no_s = novel_no.to_string();
        let mut form: Vec<(&str, &str)> = vec![("cmd", cmd), ("novel_no", &novel_no_s)];
        // Build `episode_arr[0]=N1&episode_arr[1]=N2` entries.
        let ep_strings: Vec<String> = episode_nos.iter().map(|n| n.to_string()).collect();
        let ep_keys: Vec<String> = (0..episode_nos.len())
            .map(|i| format!("episode_arr[{}]", i))
            .collect();
        for (k, v) in ep_keys.iter().zip(ep_strings.iter()) {
            form.push((k.as_str(), v.as_str()));
        }
        let resp = self.post_form(&url, &form).await?;
        let body = resp.text().await.map_err(Error::Http)?;
        parse_list::<EpisodeViewCount>(&body, "list")
    }

    // -----------------------------------------------------------------------
    // Novel alarm toggle
    // -----------------------------------------------------------------------

    /// `POST /proc/novel_alarm` — toggle alarm subscription for a novel.
    ///
    /// Returns `Toggle::Login` when unauthenticated; returns `On`/`Off` when the
    /// subscription was toggled.
    pub async fn toggle_novel_alarm(&self, novel_no: u64) -> Result<Toggle> {
        let url = format!("{}/proc/novel_alarm", self.base_url);
        let novel_no_s = novel_no.to_string();
        let resp = self.post_form(&url, &[("novel_no", &novel_no_s)]).await?;
        let body = resp.text().await.map_err(Error::Http)?;
        Toggle::parse(&body)
    }

    /// `POST /proc/novel_like` — toggle favourite (선호) for a novel.
    ///
    /// Requires `LOGINKEY` and a CSRF token.
    pub async fn toggle_novel_like(&self, novel_no: u64) -> Result<Toggle> {
        self.require_auth()?;
        let csrf = self.csrf.as_deref().unwrap_or("");
        let url = format!("{}/proc/novel_like", self.base_url);
        let novel_no_s = novel_no.to_string();
        let resp = self
            .post_form(&url, &[("novel_no", &novel_no_s), ("csrf", csrf)])
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;
        Toggle::parse(&body)
    }

    // -----------------------------------------------------------------------
    // Novel curation
    // -----------------------------------------------------------------------

    /// `GET /proc/novel_curation?cmd=writer_other_novel&mem_no=<M>&novel_no=<N>&page=<P>`
    ///
    /// Returns other novels by the same author.
    pub async fn get_writer_other_novels(
        &self,
        mem_no: u64,
        novel_no: u64,
        page: u32,
    ) -> Result<Vec<Novel>> {
        let url = format!(
            "{}/proc/novel_curation?cmd=writer_other_novel&mem_no={}&novel_no={}&page={}",
            self.base_url, mem_no, novel_no, page
        );
        let resp = self.get(&url).await?;
        let body = resp.text().await.map_err(Error::Http)?;

        #[derive(Deserialize)]
        struct WriterOther {
            list: Vec<Novel>,
        }
        let raw = parse_base::<Value>(&body)?;
        let val = raw
            .extra
            .get("writer_other_novel")
            .ok_or_else(|| Error::parse("missing `writer_other_novel` in curation response"))?;
        let inner: WriterOther = serde_json::from_value(val.clone()).map_err(Error::Json)?;
        Ok(inner.list)
    }

    /// `GET /proc/novel_curation?cmd=epi_list_curation&main_genre=<G>&novel_no=<N>`
    ///
    /// Returns genre-based episode viewer recommendations.
    pub async fn get_epi_list_curation(&self, main_genre: u8, novel_no: u64) -> Result<Vec<Novel>> {
        let url = format!(
            "{}/proc/novel_curation?cmd=epi_list_curation&main_genre={}&novel_no={}",
            self.base_url, main_genre, novel_no
        );
        let resp = self.get(&url).await?;
        let body = resp.text().await.map_err(Error::Http)?;
        parse_list::<Novel>(&body, "curation_list")
    }
}
