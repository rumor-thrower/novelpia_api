//! Serde structs mirroring the Novelpia OpenAPI schema components.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Primitive helpers
// ---------------------------------------------------------------------------

/// Deserialise a field that the server sends as either an integer or a quoted
/// integer string — common in Novelpia's API (e.g. `novel_no`, `status`).
pub(crate) mod serde_u64_or_str {
    use serde::{Deserializer, de};
    use std::fmt;

    struct V;
    impl de::Visitor<'_> for V {
        type Value = u64;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("u64 or string-encoded u64")
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
            Ok(v)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
            u64::try_from(v).map_err(de::Error::custom)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
            v.parse().map_err(de::Error::custom)
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        d.deserialize_any(V)
    }
}

// ---------------------------------------------------------------------------
// Novelpia serial-publication status
// ---------------------------------------------------------------------------

/// Human-readable serialisation status derived from `novel_live`, `is_del`,
/// and `is_complete`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerialStatus {
    /// 연재 중 (novel_live=0, is_del=0, is_complete=0)
    Ongoing,
    /// 연재 지연 (novel_live=1)
    Delayed,
    /// 연재 중단 (novel_live=2)
    Hiatus,
    /// 완결 (is_complete=1)
    Complete,
    /// 삭제됨 (is_del=1)
    Deleted,
}

impl SerialStatus {
    pub fn from_flags(novel_live: u8, is_del: u8, is_complete: u8) -> Self {
        if is_del != 0 {
            SerialStatus::Deleted
        } else if is_complete != 0 {
            SerialStatus::Complete
        } else {
            match novel_live {
                1 => SerialStatus::Delayed,
                2 => SerialStatus::Hiatus,
                _ => SerialStatus::Ongoing,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Novel
// ---------------------------------------------------------------------------

/// Novelpia 소설(작품) 정보.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Novel {
    #[serde(deserialize_with = "serde_u64_or_str::deserialize")]
    pub novel_no: u64,
    pub novel_name: String,
    #[serde(default)]
    pub novel_story: Option<String>,
    #[serde(default)]
    pub writer_nick: Option<String>,
    #[serde(default)]
    pub mem_nick: Option<String>,
    #[serde(default)]
    pub mem_no: Option<u64>,
    #[serde(default)]
    pub main_genre: Option<u8>,
    #[serde(default)]
    pub novel_type: Option<u8>,
    #[serde(default)]
    pub novel_age: Option<u8>,
    #[serde(default)]
    pub novel_live: Option<u8>,
    #[serde(default)]
    pub is_del: Option<u8>,
    #[serde(default)]
    pub is_complete: Option<u8>,
    #[serde(default)]
    pub count_view: Option<u64>,
    #[serde(default)]
    pub count_good: Option<u64>,
    #[serde(default)]
    pub count_like: Option<u64>,
    #[serde(default)]
    pub count_alarm: Option<u64>,
    #[serde(default)]
    pub count_book: Option<u64>,
    #[serde(default)]
    pub novel_thumb: Option<String>,
    #[serde(default)]
    pub reg_date: Option<String>,
    #[serde(default)]
    pub last_write_date: Option<String>,
}

impl Novel {
    /// Derive a [`SerialStatus`] from the status flags on this record.
    pub fn serial_status(&self) -> SerialStatus {
        SerialStatus::from_flags(
            self.novel_live.unwrap_or(0),
            self.is_del.unwrap_or(0),
            self.is_complete.unwrap_or(0),
        )
    }
}

// ---------------------------------------------------------------------------
// Episode view count
// ---------------------------------------------------------------------------

/// 특정 회차의 조회수 항목.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeViewCount {
    pub episode_no: u64,
    /// 조회수 (쉼표 구분 숫자 문자열, e.g. "1,057")
    pub count_view: String,
}

impl EpisodeViewCount {
    /// Parse `count_view` into a plain `u64` (stripping commas).
    pub fn count_view_int(&self) -> Option<u64> {
        self.count_view.replace(',', "").parse().ok()
    }
}

// ---------------------------------------------------------------------------
// Member / user profile
// ---------------------------------------------------------------------------

/// 회원 공개 프로필 통계 (`get_member_view` 모드 응답 내 `result`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberView {
    pub mem_no: u64,
    #[serde(default)]
    pub view_comment: Option<u8>,
    #[serde(default)]
    pub view_like_novel: Option<u8>,
    #[serde(default)]
    pub view_like_hash: Option<u8>,
    #[serde(default)]
    pub view_sponsor: Option<u8>,
    #[serde(default)]
    pub episode_cnt: Option<u64>,
    #[serde(default)]
    pub comment_cnt: Option<u64>,
}

/// 차단 여부 확인 결과 (`get_user_block_chk` 모드 응답 내 `result`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberBlockChk {
    pub idx: u64,
    pub mem_no: u64,
    pub target_mem_no: u64,
    pub regdate: String,
}

// ---------------------------------------------------------------------------
// Novel review item
// ---------------------------------------------------------------------------

/// 소설 리뷰(작품평) 한 건.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelReviewItem {
    #[serde(default)]
    pub board_no: Option<u64>,
    pub content_no: u64,
    #[serde(default)]
    pub content_cate: Option<u64>,
    pub content_subject: String,
    #[serde(default)]
    pub content_link: Option<String>,
    #[serde(default)]
    pub content_image: Option<String>,
    #[serde(default)]
    pub content_memo_linkno: Option<u64>,
    #[serde(default)]
    pub content_adult: Option<u8>,
    /// 작성자 닉네임 (HTML 태그 포함 가능)
    pub mem_nick: String,
    pub mem_no: u64,
    #[serde(default)]
    pub count_view: Option<u64>,
    #[serde(default)]
    pub count_comment: Option<u64>,
    #[serde(default)]
    pub count_good: Option<u64>,
    pub content_regdate: String,
    pub novel_name: String,
    pub novel_no: u64,
    #[serde(default)]
    pub novel_thumb: Option<String>,
}

// ---------------------------------------------------------------------------
// Episode list row (parsed from HTML fragment)
// ---------------------------------------------------------------------------

/// 회차 목록 테이블의 한 행.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeListRow {
    pub episode_no: u64,
    pub title: String,
    /// 무료 여부 (free=true, plus=false)
    pub is_free: bool,
    /// 19금 여부
    #[serde(default)]
    pub is_adult: bool,
    #[serde(default)]
    pub reg_date: Option<String>,
    /// 조회수 텍스트 (e.g. "1,234")
    #[serde(default)]
    pub count_view: Option<String>,
}
