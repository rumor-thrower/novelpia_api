//! `/proc/episode_list`, `/proc/viewer_data`, and `/proc/board_option` endpoint implementations.

use scraper::{Html, Selector};

use crate::{
    client::Toggle,
    error::{Error, Result},
    models::EpisodeListRow,
    response::parse_viewer_lines,
    Client,
};

// ---------------------------------------------------------------------------
// Episode list (HTML fragment)
// ---------------------------------------------------------------------------

impl Client {
    /// `POST /proc/episode_list` — returns an HTML fragment containing the
    /// episode table for `novel_no`.
    ///
    /// `sort` should be `"DOWN"` (newest first, default) or `"UP"` (oldest first).
    /// `page` is 0-indexed.
    pub async fn get_episode_list(
        &self,
        novel_no: u64,
        sort: &str,
        page: u32,
    ) -> Result<Vec<EpisodeListRow>> {
        let url = format!("{}/proc/episode_list", self.base_url);
        let novel_no_s = novel_no.to_string();
        let page_s = page.to_string();
        let resp = self
            .post_form(
                &url,
                &[
                    ("novel_no", &novel_no_s),
                    ("sort", sort),
                    ("page", &page_s),
                ],
            )
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;
        parse_episode_list_html(&body)
    }

    // -----------------------------------------------------------------------
    // Viewer data (episode text)
    // -----------------------------------------------------------------------

    /// `POST /proc/viewer_data/{ep}` — fetch episode text lines.
    ///
    /// The `Referer: .../viewer/{ep}` header is mandatory; the client sends it
    /// automatically. Returns `Error::NotAccessible` on paywall or auth failure.
    pub async fn get_viewer_data(&self, episode_no: u64) -> Result<Vec<String>> {
        let url = format!("{}/proc/viewer_data/{}", self.base_url, episode_no);
        let referer = format!("{}/viewer/{}", self.base_url, episode_no);
        let resp = self
            .post_form_with_referer(&url, &[("size", "14")], &referer)
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;
        if body.trim().is_empty() {
            return Err(Error::NotAccessible(
                "viewer returned empty body (paywall or auth required)".into(),
            ));
        }
        parse_viewer_lines(&body)
    }

    // -----------------------------------------------------------------------
    // Board option (novel vote)
    // -----------------------------------------------------------------------

    /// `POST /proc/board_option` `option=vote_novel` — toggle episode recommendation.
    ///
    /// Requires `LOGINKEY` and a CSRF token.
    pub async fn toggle_episode_vote(&self, episode_no: u64) -> Result<Toggle> {
        self.require_auth()?;
        let csrf = self.csrf.as_deref().unwrap_or("");
        let url = format!("{}/proc/board_option", self.base_url);
        let ep_s = episode_no.to_string();
        let resp = self
            .post_form(
                &url,
                &[
                    ("option", "vote_novel"),
                    ("value", &ep_s),
                    ("csrf", csrf),
                ],
            )
            .await?;
        let body = resp.text().await.map_err(Error::Http)?;
        Toggle::parse(&body)
    }
}

// ---------------------------------------------------------------------------
// HTML fragment parser (episode list table)
// ---------------------------------------------------------------------------

/// Parse the `POST /proc/episode_list` HTML fragment into typed rows.
pub(crate) fn parse_episode_list_html(html: &str) -> Result<Vec<EpisodeListRow>> {
    let document = Html::parse_fragment(html);

    // Each episode row has class "ep_style" (or "ep_style2" for free episodes in some layouts).
    let row_sel = Selector::parse("li.ep_style, li.ep_style2, li.ep_style3")
        .map_err(|e| Error::parse(format!("episode row selector parse error: {:?}", e)))?;
    let title_sel = Selector::parse("b.ep_title, .b_title, .ep_title")
        .map_err(|e| Error::parse(format!("title selector parse error: {:?}", e)))?;
    let date_sel = Selector::parse(".ep_date, .p_date")
        .map_err(|e| Error::parse(format!("date selector parse error: {:?}", e)))?;
    let view_sel = Selector::parse(".ep_view, .p_view, .count_view")
        .map_err(|e| Error::parse(format!("view selector parse error: {:?}", e)))?;

    let mut rows = Vec::new();

    for row in document.select(&row_sel) {
        // episode_no is stored in a data attribute or embedded in an onclick/href.
        // The episode number lives in: data-episode-no, data-epno, or onclick="epView(N)"
        let ep_no = extract_episode_no(&row);
        let Some(episode_no) = ep_no else { continue };

        let title = row
            .select(&title_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_owned())
            .unwrap_or_default();

        // Free episodes typically carry a "FREE" badge or lack a coin indicator.
        let is_free = !row.inner_html().contains("class=\"ep_type\"")
            && !row.inner_html().contains("plus_ep");

        let reg_date = row
            .select(&date_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_owned());

        let count_view = row
            .select(&view_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_owned());

        rows.push(EpisodeListRow {
            episode_no,
            title,
            is_free,
            reg_date: reg_date.filter(|s| !s.is_empty()),
            count_view: count_view.filter(|s| !s.is_empty()),
        });
    }

    Ok(rows)
}

fn extract_episode_no(el: &scraper::ElementRef<'_>) -> Option<u64> {
    // 1. data-episode-no attribute
    if let Some(v) = el.value().attr("data-episode-no") {
        if let Ok(n) = v.trim().parse() {
            return Some(n);
        }
    }
    // 2. data-epno attribute
    if let Some(v) = el.value().attr("data-epno") {
        if let Ok(n) = v.trim().parse() {
            return Some(n);
        }
    }
    // 3. onclick="epView(N)" or onclick="location.href='/viewer/N'"
    if let Some(onclick) = el.value().attr("onclick") {
        if let Some(n) = parse_number_from_js(onclick) {
            return Some(n);
        }
    }
    // 4. Descendant <a href="/viewer/N"> or any onclick on child elements
    let html = el.inner_html();
    if let Some(pos) = html.find("/viewer/") {
        let after = &html[pos + 8..];
        let end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
        if let Ok(n) = after[..end].parse() {
            return Some(n);
        }
    }
    None
}

fn parse_number_from_js(s: &str) -> Option<u64> {
    // Extract first run of digits that looks like an episode no (≥3 digits).
    let mut digits = String::new();
    let mut in_digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            in_digits = true;
        } else if in_digits {
            if digits.len() >= 3 {
                break;
            }
            digits.clear();
            in_digits = false;
        }
    }
    if digits.len() >= 3 {
        digits.parse().ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_html_returns_empty_vec() {
        let rows = parse_episode_list_html("<ul></ul>").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn parse_episode_list_with_data_attribute() {
        let html = r#"
        <ul>
            <li class="ep_style" data-episode-no="1234">
                <b class="ep_title">1화 제목</b>
                <span class="ep_date">2024.01.01</span>
                <span class="ep_view">1,234</span>
            </li>
        </ul>
        "#;
        let rows = parse_episode_list_html(html).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].episode_no, 1234);
        assert_eq!(rows[0].title, "1화 제목");
        assert_eq!(rows[0].reg_date.as_deref(), Some("2024.01.01"));
        assert_eq!(rows[0].count_view.as_deref(), Some("1,234"));
    }

    #[test]
    fn parse_episode_list_viewer_href() {
        let html = r#"
        <ul>
            <li class="ep_style">
                <a href="/viewer/5678"><b class="ep_title">2화</b></a>
            </li>
        </ul>
        "#;
        let rows = parse_episode_list_html(html).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].episode_no, 5678);
    }
}
