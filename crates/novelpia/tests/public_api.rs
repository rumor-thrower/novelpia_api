//! Integration tests for the `novelpia` crate.
//!
//! Tests that rely solely on in-memory fixtures run unconditionally.
//! Tests that hit the live Novelpia servers are marked `#[ignore]` and require
//! network access; run them with `cargo test -- --ignored`.
//!
//! Live tests that need authentication additionally read credentials from
//! environment variables (`LOGINKEY`, `CSRF_TOKEN`).

use novelpia::{
    parse_episode_list_html,
    response::{parse_base, parse_viewer_lines, BaseResponse},
    models::{EpisodeViewCount, NovelReviewItem},
    error::Error,
};

// ---------------------------------------------------------------------------
// Offline: pipe-response parser (Toggle)
// ---------------------------------------------------------------------------

#[test]
fn toggle_parse_on() {
    use novelpia::Toggle;
    let t = Toggle::parse("on|12345\n").unwrap();
    assert!(matches!(t, Toggle::On(12345)));
}

#[test]
fn toggle_parse_off() {
    use novelpia::Toggle;
    let t = Toggle::parse("off|12345").unwrap();
    assert!(matches!(t, Toggle::Off(12345)));
}

#[test]
fn toggle_parse_login() {
    use novelpia::Toggle;
    let t = Toggle::parse("login|0||").unwrap();
    assert!(matches!(t, Toggle::Login));
}

// ---------------------------------------------------------------------------
// Offline: BaseResponse status as string or int
// ---------------------------------------------------------------------------

#[test]
fn base_response_status_as_string() {
    let json = r#"{"status":"200","errmsg":""}"#;
    let r: BaseResponse = parse_base(json).unwrap();
    assert_eq!(r.status, 200);
}

#[test]
fn base_response_status_as_int() {
    let json = r#"{"status":200,"errmsg":""}"#;
    let r: BaseResponse = parse_base(json).unwrap();
    assert_eq!(r.status, 200);
}

#[test]
fn base_response_auth_error_401() {
    let json = r#"{"status":401,"errmsg":"login required"}"#;
    let r: BaseResponse = parse_base(json).unwrap();
    let err = r.into_result().unwrap_err();
    assert!(matches!(err, Error::AuthRequired));
}

// ---------------------------------------------------------------------------
// Offline: viewer &nbsp; → empty string
// ---------------------------------------------------------------------------

#[test]
fn viewer_nbsp_becomes_empty_line() {
    let json = r#"[{"text":"Hello"},{"text":"&nbsp;"},{"text":"World"}]"#;
    let lines = parse_viewer_lines(json).unwrap();
    assert_eq!(lines, vec!["Hello", "", "World"]);
}

#[test]
fn viewer_html_body_returns_not_accessible() {
    let html = "<html><body><script>alert('로그인')</script></body></html>";
    let err = parse_viewer_lines(html).unwrap_err();
    assert!(matches!(err, Error::NotAccessible(_)));
}

// ---------------------------------------------------------------------------
// Offline: episode_list HTML fragment parser
// ---------------------------------------------------------------------------

#[test]
fn episode_list_empty_fragment() {
    let rows = parse_episode_list_html("<ul></ul>").unwrap();
    assert!(rows.is_empty());
}

#[test]
fn episode_list_data_attribute() {
    let html = r#"
    <ul>
        <li class="ep_style" data-episode-no="9001">
            <b class="ep_title">프롤로그</b>
            <span class="ep_date">2024.03.15</span>
            <span class="ep_view">2,048</span>
        </li>
    </ul>
    "#;
    let rows = parse_episode_list_html(html).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].episode_no, 9001);
    assert_eq!(rows[0].title, "프롤로그");
    assert_eq!(rows[0].reg_date.as_deref(), Some("2024.03.15"));
    assert_eq!(rows[0].count_view.as_deref(), Some("2,048"));
}

#[test]
fn episode_list_viewer_href_fallback() {
    let html = r#"
    <ul>
        <li class="ep_style">
            <a href="/viewer/12345"><b class="ep_title">1화</b></a>
        </li>
    </ul>
    "#;
    let rows = parse_episode_list_html(html).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].episode_no, 12345);
}

#[test]
fn episode_list_ep_style2_and_ep_style3() {
    let html = r#"
    <ul>
        <li class="ep_style2" data-episode-no="100">
            <b class="ep_title">무료</b>
        </li>
        <li class="ep_style3" data-episode-no="101">
            <b class="ep_title">플러스</b>
        </li>
    </ul>
    "#;
    let rows = parse_episode_list_html(html).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].episode_no, 100);
    assert_eq!(rows[1].episode_no, 101);
}

#[test]
fn episode_list_skips_row_without_episode_no() {
    let html = r#"
    <ul>
        <li class="ep_style">
            <b class="ep_title">no episode number anywhere</b>
        </li>
        <li class="ep_style" data-episode-no="42">
            <b class="ep_title">valid</b>
        </li>
    </ul>
    "#;
    let rows = parse_episode_list_html(html).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].episode_no, 42);
}

// ---------------------------------------------------------------------------
// Offline: EpisodeViewCount comma-formatted int parsing
// ---------------------------------------------------------------------------

#[test]
fn episode_view_count_parse_comma_number() {
    let item = EpisodeViewCount {
        episode_no: 100,
        count_view: "12,345".into(),
    };
    assert_eq!(item.count_view_int(), Some(12345));
}

#[test]
fn episode_view_count_parse_plain_number() {
    let item = EpisodeViewCount {
        episode_no: 100,
        count_view: "999".into(),
    };
    assert_eq!(item.count_view_int(), Some(999));
}

// ---------------------------------------------------------------------------
// Offline: NovelReviewItem serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn novel_review_item_deserialises() {
    let json = r#"{
        "content_no": 1,
        "content_subject": "재밌어요",
        "mem_nick": "독자",
        "mem_no": 42,
        "content_regdate": "2024-01-01",
        "novel_name": "테스트 소설",
        "novel_no": 99
    }"#;
    let item: NovelReviewItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.content_no, 1);
    assert_eq!(item.novel_no, 99);
    assert_eq!(item.novel_name, "테스트 소설");
}

// ---------------------------------------------------------------------------
// Offline: EmoticonGroup / EmoticonItem serde
// ---------------------------------------------------------------------------

#[test]
fn emoticon_group_deserialises() {
    use novelpia::EmoticonGroup;
    let json = r#"{"emoticon_group":1700,"emoticon_group_name":"천재 마법사"}"#;
    let g: EmoticonGroup = serde_json::from_str(json).unwrap();
    assert_eq!(g.emoticon_group, 1700);
    assert_eq!(g.emoticon_group_name.as_deref(), Some("천재 마법사"));
    assert!(g.is_hide.is_none());
}

#[test]
fn emoticon_group_with_is_hide() {
    use novelpia::EmoticonGroup;
    let json = r#"{"emoticon_group":5,"emoticon_group_name":"숨김","is_hide":1}"#;
    let g: EmoticonGroup = serde_json::from_str(json).unwrap();
    assert_eq!(g.is_hide, Some(1));
}

#[test]
fn emoticon_item_deserialises() {
    use novelpia::EmoticonItem;
    let json = r#"{"emoticon_no":77,"emoticon_group":5,"emoticon_img":"img/emt/77.png"}"#;
    let item: EmoticonItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.emoticon_no, 77);
    assert_eq!(item.emoticon_group, Some(5));
}

// ---------------------------------------------------------------------------
// Live (network): public read-only endpoints
// Run with: cargo test -- --ignored
// ---------------------------------------------------------------------------

fn make_client() -> novelpia::Client {
    novelpia::ClientBuilder::default()
        .delay_ms(500, 1000)
        .build()
}

fn make_auth_client() -> novelpia::Client {
    let key = std::env::var("LOGINKEY").expect("LOGINKEY env var required for auth tests");
    let csrf = std::env::var("CSRF_TOKEN").unwrap_or_default();
    novelpia::ClientBuilder::default()
        .delay_ms(500, 1000)
        .login_key(key)
        .csrf(csrf)
        .build()
}

#[tokio::test]
#[ignore]
async fn live_get_novel_review_list() {
    let client = make_client();
    let reviews = client.get_novel_review_list(97958).await.unwrap();
    assert!(!reviews.is_empty(), "expected at least one review for novel 97958");
    assert!(!reviews[0].novel_name.is_empty());
}

#[tokio::test]
#[ignore]
async fn live_get_episode_view_counts() {
    let client = make_client();
    let counts = client
        .get_episode_view_counts(23, &[100, 200], false)
        .await
        .unwrap();
    assert_eq!(counts.len(), 2);
}

#[tokio::test]
#[ignore]
async fn live_get_episode_list() {
    let client = make_client();
    let rows = client.get_episode_list(23, "DOWN", 0).await.unwrap();
    assert!(!rows.is_empty(), "expected at least one episode row for novel 23");
    assert!(rows[0].episode_no > 0);
}

#[tokio::test]
#[ignore]
async fn live_get_viewer_data_free_episode() {
    // This test requires a known free episode number.
    // Replace with a known-free episode number for the target novel.
    let ep_no: u64 = std::env::var("TEST_FREE_EPISODE")
        .ok()
        .and_then(|s| s.parse().ok())
        .expect("set TEST_FREE_EPISODE to a known-free episode number");
    let client = make_client();
    let lines = client.get_viewer_data(ep_no).await.unwrap();
    assert!(!lines.is_empty(), "expected non-empty episode text");
}

#[tokio::test]
#[ignore]
async fn live_get_member2() {
    let client = make_client();
    // Use a known public member number.
    let val = client.get_member2(1).await.unwrap();
    assert!(val.is_object() || val.is_array());
}

#[tokio::test]
#[ignore]
async fn live_get_member_view() {
    let client = make_client();
    let view = client.get_member_view(1).await.unwrap();
    assert_eq!(view.mem_no, 1);
}

#[tokio::test]
#[ignore]
async fn live_get_alarm_cnt_unauthenticated() {
    let client = make_client();
    let cnt = client.get_alarm_cnt().await.unwrap();
    // Unauthenticated should return 0, not an error.
    assert_eq!(cnt, 0);
}

#[tokio::test]
#[ignore]
async fn live_get_member_plus_event_list() {
    let client = make_client();
    let val = client.get_member_plus_event_list().await.unwrap();
    assert!(val.is_object() || val.is_array());
}

#[tokio::test]
#[ignore]
async fn live_get_writer_other_novels() {
    let client = make_client();
    let novels = client.get_writer_other_novels(1, 23, 1).await.unwrap();
    // May be empty if author has only one novel; just check it doesn't error.
    let _ = novels;
}

// ---------------------------------------------------------------------------
// Live + auth: endpoints that require LOGINKEY
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_auth_get_alarm_cnt() {
    let client = make_auth_client();
    let cnt = client.get_alarm_cnt().await.unwrap();
    // Just assert it's a plausible number (could be 0).
    let _ = cnt;
}

#[tokio::test]
#[ignore]
async fn live_auth_get_user_emoticon_group() {
    let client = make_auth_client();
    let groups = client.get_user_emoticon_group(false).await.unwrap();
    let _ = groups;
}

#[tokio::test]
#[ignore]
async fn live_auth_get_member_favorite_novel() {
    let client = make_auth_client();
    let favs = client.get_member_favorite_novel().await.unwrap();
    let _ = favs;
}

#[tokio::test]
#[ignore]
async fn live_auth_get_user_block_list() {
    let client = make_auth_client();
    let mem_no: u64 = std::env::var("TEST_MEM_NO")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let blocked = client.get_user_block_list(mem_no).await.unwrap();
    let _ = blocked;
}
