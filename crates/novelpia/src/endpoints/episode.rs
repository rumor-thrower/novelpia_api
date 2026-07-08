//! `/proc/episode_list`, `/proc/viewer_data`, and `/proc/board_option` endpoint implementations.

use scraper::{Html, Selector};

use crate::{
    Client,
    client::Toggle,
    error::{Error, Result},
    models::EpisodeListRow,
    response::parse_viewer_lines,
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
                &[("novel_no", &novel_no_s), ("sort", sort), ("page", &page_s)],
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
                &[("option", "vote_novel"), ("value", &ep_s), ("csrf", csrf)],
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
pub fn parse_episode_list_html(html: &str) -> Result<Vec<EpisodeListRow>> {
    let document = Html::parse_fragment(html);

    // The live episode table renders each row as `<tr class="ep_style5">`.
    // Older/alternate layouts used `<li class="ep_style">`; accept both.
    let row_sel = Selector::parse("tr.ep_style5, li.ep_style, li.ep_style2, li.ep_style3")
        .map_err(|e| Error::parse(format!("episode row selector parse error: {:?}", e)))?;
    // The title lives in the first <b> of the clickable cell; the trailing view
    // metadata lives inside `.ep_style2` so restrict title lookup to the outer
    // <b> tags. `b.ep_title` covers the legacy fixture.
    let title_sel = Selector::parse("b.ep_title, .b_title, .ep_title, b")
        .map_err(|e| Error::parse(format!("title selector parse error: {:?}", e)))?;
    // Free/plus/adult badge markers.
    let free_sel = Selector::parse("span.b_free")
        .map_err(|e| Error::parse(format!("free selector parse error: {:?}", e)))?;
    let adult_sel = Selector::parse("span.b_19")
        .map_err(|e| Error::parse(format!("adult selector parse error: {:?}", e)))?;
    // Any badge (free/plus/adult) whose leading text should be stripped from
    // the title.
    let badge_sel = Selector::parse("span.b_free, span.b_plus, span.b_19")
        .map_err(|e| Error::parse(format!("badge selector parse error: {:?}", e)))?;
    let legacy_date_sel = Selector::parse(".ep_date, .p_date")
        .map_err(|e| Error::parse(format!("date selector parse error: {:?}", e)))?;
    // Live view count.
    let view_sel = Selector::parse("span.episode_count_view, .ep_view, .p_view, .count_view")
        .map_err(|e| Error::parse(format!("view selector parse error: {:?}", e)))?;

    let mut rows = Vec::new();

    for row in document.select(&row_sel) {
        // episode_no is stored in a data attribute or embedded in an onclick/href.
        // The episode number lives in: data-episode-no, data-epno, or onclick="epView(N)"
        let ep_no = extract_episode_no(&row);
        let Some(episode_no) = ep_no else { continue };

        // Title: first matching <b>, with any leading badge text (e.g. "무료",
        // "19") stripped. A row may carry multiple badges (free + adult), each
        // a nested <span> rendered before the title text, so peel them off one
        // at a time in document order.
        let title = row
            .select(&title_sel)
            .next()
            .map(|el| {
                let full: String = el.text().collect();
                let mut remaining = full.trim_start();
                for badge in el.select(&badge_sel) {
                    let badge_text: String = badge.text().collect();
                    let badge_text = badge_text.trim();
                    if badge_text.is_empty() {
                        continue;
                    }
                    match remaining.strip_prefix(badge_text) {
                        Some(rest) => remaining = rest.trim_start(),
                        None => break,
                    }
                }
                remaining.trim().to_owned()
            })
            .unwrap_or_default();

        // Free episodes carry a `span.b_free` badge (무료); plus/adult episodes
        // use `b_plus`/`b_19` instead.
        let is_free = row.select(&free_sel).next().is_some();
        let is_adult = row.select(&adult_sel).next().is_some();

        // Date: legacy `.ep_date`, else the `NN.NN.NN` run in the live markup.
        let reg_date = row
            .select(&legacy_date_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| extract_reg_date(&row));

        let count_view = row
            .select(&view_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_owned());

        rows.push(EpisodeListRow {
            episode_no,
            title,
            is_free,
            is_adult,
            reg_date: reg_date.filter(|s| !s.is_empty()),
            count_view: count_view.filter(|s| !s.is_empty()),
        });
    }

    Ok(rows)
}

/// Extract a `YY.MM.DD` date from a row's text content.
fn extract_reg_date(row: &scraper::ElementRef<'_>) -> Option<String> {
    let text: String = row.text().collect();
    // Scan for the first `NN.NN.NN` pattern.
    let bytes = text.as_bytes();
    let is_d = |b: u8| b.is_ascii_digit();
    let mut i = 0;
    while i + 8 <= bytes.len() {
        if is_d(bytes[i])
            && is_d(bytes[i + 1])
            && bytes[i + 2] == b'.'
            && is_d(bytes[i + 3])
            && is_d(bytes[i + 4])
            && bytes[i + 5] == b'.'
            && is_d(bytes[i + 6])
            && is_d(bytes[i + 7])
        {
            return Some(text[i..i + 8].to_owned());
        }
        i += 1;
    }
    None
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
        let end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
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
    fn parse_episode_list_live_table_markup() {
        // Trimmed from a live `/proc/episode_list` response.
        let html = r#"
        <table id="episode_table">
            <tr class="ep_style5" data-episode-no="1134">
                <td class=""><div class="episode_view_1134"></div></td>
                <td class="font12"><b>
                    <span class="b_free s_inv">무료</span>
                    <i class="icon ion-bookmark" id="bookmark_1134"></i>#01_마녀도시의 노예</b> <br>
                    <div class="ep_style2"><font class="font11">
                        <span>EP.1</span>
                        <span><i class="icon ion-document-text"></i> 3,867
                            <span class="episode_show"><i class="icon ion-android-people"></i>
                                <span class="episode_count_view novel_count_view_1134">42</span>
                            </span>
                            <i class="icon ion-chatbox-working"></i> 226
                            <i class="icon ion-thumbsup"></i> 3,902<br>
                            <b>21.01.07</b>
                        </span>
                    </font></div>
                </td>
                <td class="ep_style3"></td>
            </tr>
        </table>
        "#;
        let rows = parse_episode_list_html(html).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].episode_no, 1134);
        assert_eq!(rows[0].title, "#01_마녀도시의 노예");
        assert!(rows[0].is_free);
        assert_eq!(rows[0].reg_date.as_deref(), Some("21.01.07"));
        assert_eq!(rows[0].count_view.as_deref(), Some("42"));
    }

    #[test]
    fn parse_episode_list_title_keeps_badge_text_inside_title() {
        // The badge text ("무료") also appears inside the actual title. Only
        // the leading badge occurrence should be stripped, not this one.
        let html = r#"
        <table id="episode_table">
            <tr class="ep_style5" data-episode-no="42">
                <td class=""><div class="episode_view_42"></div></td>
                <td class="font12"><b>
                    <span class="b_free s_inv">무료</span>
                    <i class="icon ion-bookmark" id="bookmark_42"></i>이 악마들은 무료로 해줍니다.</b> <br>
                </td>
                <td class="ep_style3"></td>
            </tr>
        </table>
        "#;
        let rows = parse_episode_list_html(html).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "이 악마들은 무료로 해줍니다.");
    }

    #[test]
    fn parse_episode_list_title_with_plus_badge_and_matching_word() {
        // The leading `b_plus` badge is stripped like `b_free`, but the
        // trailing "PLUS!!" inside the actual title text is left intact.
        let html = r#"
        <table id="episode_table">
            <tr class="ep_style5" data-episode-no="99">
                <td class=""><div class="episode_view_99"></div></td>
                <td class="font12"><b>
                    <span class="b_plus s_inv">PLUS</span>
                    <i class="icon ion-bookmark" id="bookmark_99"></i>뭐든지 가능한 유시아 아가씨! PLUS!!</b> <br>
                </td>
                <td class="ep_style3"></td>
            </tr>
        </table>
        "#;
        let rows = parse_episode_list_html(html).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "뭐든지 가능한 유시아 아가씨! PLUS!!");
        assert!(!rows[0].is_free);
    }

    #[test]
    fn parse_episode_list_strips_multiple_leading_badges() {
        // Live markup for adult (19금) free episodes renders two leading
        // badges back-to-back: `b_free` ("무료") then `b_19` ("19"), followed
        // by `&nbsp;` and a bookmark icon before the actual title text. Only
        // the first badge was previously stripped, leaving "19" glued onto
        // the title.
        let html = r#"
        <table id="episode_table">
            <tr class="ep_style5" data-episode-no="7146">
                <td class=""><div class="episode_view_7146"></div></td>
                <td class="font12"><b>
                    <span class="b_free s_inv">무료</span>
                    <span class="b_19 s_inv">19</span>&nbsp;
                    <i class="icon ion-bookmark" id="bookmark_7146"></i>001. 능력 각성</b> <br>
                </td>
                <td class="ep_style3"></td>
            </tr>
        </table>
        "#;
        let rows = parse_episode_list_html(html).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "001. 능력 각성");
        assert!(rows[0].is_free);
        assert!(rows[0].is_adult);
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
