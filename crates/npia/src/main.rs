//! `npia` — command-line interface for the unofficial Novelpia API client.

use std::path::{Path, PathBuf};

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "npia", about = "Unofficial Novelpia API client CLI", version)]
struct Cli {
    /// LOGINKEY cookie value (or set env LOGINKEY)
    #[arg(long, env = "LOGINKEY", global = true)]
    login_key: Option<String>,

    /// CSRF token for write endpoints (or set env CSRF_TOKEN)
    #[arg(long, env = "CSRF_TOKEN", global = true)]
    csrf: Option<String>,

    /// Minimum inter-request delay in milliseconds
    #[arg(long, default_value = "1000", global = true)]
    delay_min: u64,

    /// Maximum inter-request delay in milliseconds
    #[arg(long, default_value = "3000", global = true)]
    delay_max: u64,

    /// Output format
    #[arg(long, default_value = "json", global = true)]
    format: OutputFormat,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Csv,
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
enum Command {
    /// Fetch novel reviews (작품평) — GET /proc/novel?cmd=get_novel_review_list
    Reviews {
        /// Novel number
        #[arg(long)]
        novel: u64,
    },

    /// Fetch per-episode view counts (batch) — POST /proc/novel
    Views {
        /// Novel number
        #[arg(long)]
        novel: u64,

        /// Comma-separated episode numbers
        #[arg(long, value_delimiter = ',')]
        episodes: Vec<u64>,

        /// Use legacy cmd=get_episode_cnt_view instead of get_episode_count_view
        #[arg(long, action = ArgAction::SetTrue)]
        legacy: bool,
    },

    /// Fetch episode list (HTML→typed) — POST /proc/episode_list
    Episodes {
        /// Novel number
        #[arg(long)]
        novel: u64,

        /// Sort order
        #[arg(long, default_value = "DOWN")]
        sort: String,

        /// Page (0-indexed)
        #[arg(long, default_value = "0")]
        page: u32,
    },

    /// Fetch free episode text — POST /proc/viewer_data/{ep}
    Viewer {
        /// Episode number
        episode: u64,
    },

    /// Fetch member profile — POST /proc/user
    Member {
        /// Member number
        mem_no: u64,

        /// /proc/user mode
        #[arg(long, default_value = "get_member2")]
        mode: MemberMode,

        /// Print only the scalar value from the `result` field (for numeric modes like get-episode-cnt)
        #[arg(long, action = ArgAction::SetTrue)]
        scalar: bool,

        /// Filter items by field and regex: FIELD=PATTERN (e.g. --grep badge_memo="펀딩|후원")
        #[arg(long, value_name = "FIELD=PATTERN")]
        grep: Option<String>,

        /// Shorthand for --grep badge_memo=PATTERN
        #[arg(long)]
        grep_memo: Option<String>,
    },

    /// Fetch alarm count — POST /proc/alarm
    AlarmCnt,

    /// Fetch novel curation (writer other novels)
    Curation {
        /// Author member number
        #[arg(long)]
        mem_no: u64,

        /// Novel number (for context)
        #[arg(long)]
        novel: u64,

        /// Page (1-indexed)
        #[arg(long, default_value = "1")]
        page: u32,
    },

    /// Fetch emoticon groups owned by the logged-in user
    EmoticonGroups {
        /// Include is_hide flag (target=sort variant)
        #[arg(long, action = ArgAction::SetTrue)]
        sort: bool,
    },

    /// Fetch emoticons in a group owned by the logged-in user
    EmoticonItems {
        /// Emoticon group number
        group: u64,
    },

    /// Export a novel's data bundle (episodes, view counts, reviews) to CSV/JSON
    /// files — the handoff artifacts consumed by the analysis layer.
    Export {
        /// Novel number
        #[arg(long)]
        novel: u64,

        /// Output directory (created if missing)
        #[arg(long, default_value = ".")]
        out: PathBuf,

        /// Sort order for the episode list
        #[arg(long, default_value = "DOWN")]
        sort: String,

        /// Skip fetching per-episode view counts (faster, one fewer request batch)
        #[arg(long, action = ArgAction::SetTrue)]
        no_views: bool,

        /// Stop after this many episode-list pages (0 = no limit). The server
        /// paginates at ~20 episodes per page, so 50 silently truncated any
        /// novel past 1,000 episodes; default to unlimited and rely on the
        /// empty-page/repeated-page stop conditions in `fetch_all_episodes`.
        #[arg(long, default_value = "0")]
        max_pages: u32,
    },
}

#[derive(Clone, ValueEnum)]
enum MemberMode {
    GetMember2,
    GetMemberView,
    GetMemberWriterNovel,
    GetMemberBadge,
    GetMemberEmoticon,
    GetMemberStamp,
    GetMemberKeepNovel,
    GetMemberDonation,
    GetEpisodeCnt,
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let mut builder = novelpia::ClientBuilder::default().delay_ms(cli.delay_min, cli.delay_max);
    if let Some(key) = &cli.login_key {
        builder = builder.login_key(key.clone());
    }
    if let Some(csrf) = &cli.csrf {
        builder = builder.csrf(csrf.clone());
    }
    let client = builder.build();

    let result = run(&cli, &client).await;
    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run(cli: &Cli, client: &novelpia::Client) -> Result<(), Box<dyn std::error::Error>> {
    match &cli.cmd {
        Command::Reviews { novel } => {
            let items = client.get_novel_review_list(*novel).await?;
            print_output(&items, &cli.format)?;
        }

        Command::Views {
            novel,
            episodes,
            legacy,
        } => {
            let items = client
                .get_episode_view_counts(*novel, episodes, *legacy)
                .await?;
            print_output(&items, &cli.format)?;
        }

        Command::Episodes { novel, sort, page } => {
            let rows = client.get_episode_list(*novel, sort, *page).await?;
            print_output(&rows, &cli.format)?;
        }

        Command::Viewer { episode } => {
            let lines = client.get_viewer_data(*episode).await?;
            match cli.format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&lines)?);
                }
                OutputFormat::Csv => {
                    for line in &lines {
                        println!("{}", line);
                    }
                }
            }
        }

        Command::Member {
            mem_no,
            mode,
            scalar,
            grep,
            grep_memo,
        } => {
            let val: serde_json::Value = match mode {
                MemberMode::GetMember2 => client.get_member2(*mem_no).await?,
                MemberMode::GetMemberView => {
                    serde_json::to_value(client.get_member_view(*mem_no).await?)?
                }
                MemberMode::GetMemberWriterNovel => {
                    serde_json::to_value(client.get_member_writer_novel(*mem_no).await?)?
                }
                MemberMode::GetMemberBadge => client.get_member_badge(*mem_no).await?,
                MemberMode::GetMemberEmoticon => client.get_member_emoticon(*mem_no).await?,
                MemberMode::GetMemberStamp => client.get_member_stamp(*mem_no).await?,
                MemberMode::GetMemberKeepNovel => client.get_member_keep_novel(*mem_no).await?,
                MemberMode::GetMemberDonation => client.get_member_donation(*mem_no).await?,
                MemberMode::GetEpisodeCnt => client.get_episode_cnt(*mem_no).await?,
            };
            let grep_spec: Option<(&str, &str)> = if let Some(raw) = grep {
                let (field, pattern) = raw
                    .split_once('=')
                    .ok_or_else(|| format!("--grep requires FIELD=PATTERN, got: {raw}"))?;
                Some((field, pattern))
            } else {
                grep_memo
                    .as_ref()
                    .map(|pattern| ("badge_memo", pattern.as_str()))
            };
            if let Some((field, pattern)) = grep_spec {
                let re = regex::Regex::new(pattern).map_err(|e| format!("invalid pattern: {e}"))?;
                let filtered = grep_field_items(&val, field, &re);
                println!("{}", serde_json::to_string_pretty(&filtered)?);
            } else if *scalar {
                let target = val.get("result").unwrap_or(&val);
                match format_scalar(target) {
                    Some(s) => println!("{}", s),
                    None => eprintln!("note: no scalar value found in result"),
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&val)?);
            }
        }

        Command::AlarmCnt => {
            let cnt = client.get_alarm_cnt().await?;
            println!("{}", cnt);
        }

        Command::Curation {
            mem_no,
            novel,
            page,
        } => {
            let novels = client
                .get_writer_other_novels(*mem_no, *novel, *page)
                .await?;
            print_output(&novels, &cli.format)?;
        }

        Command::EmoticonGroups { sort } => {
            let groups = client.get_user_emoticon_group(*sort).await?;
            print_output(&groups, &cli.format)?;
        }

        Command::EmoticonItems { group } => {
            let items = client.get_user_emoticon(*group).await?;
            print_output(&items, &cli.format)?;
        }

        Command::Export {
            novel,
            out,
            sort,
            no_views,
            max_pages,
        } => {
            run_export(client, *novel, out, sort, *no_views, *max_pages).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Export bundle
// ---------------------------------------------------------------------------

/// Metadata written alongside the exported data files.
#[derive(Serialize)]
struct ExportManifest {
    novel_no: u64,
    exported_at: String,
    sort: String,
    episode_count: usize,
    view_count_rows: usize,
    review_count: usize,
    files: Vec<String>,
}

/// Fetch episode-list pages until an empty page is returned, `max_pages` is
/// reached (`max_pages == 0` means no limit), or the server repeats the
/// previous page (some deployments clamp an out-of-range `page` to the last
/// valid one instead of returning an empty result).
async fn fetch_all_episodes(
    client: &novelpia::Client,
    novel: u64,
    sort: &str,
    max_pages: u32,
) -> Result<Vec<novelpia::models::EpisodeListRow>, Box<dyn std::error::Error>> {
    let mut all = Vec::new();
    let mut page = 0u32;
    let mut prev_episode_nos: Option<Vec<u64>> = None;
    loop {
        if max_pages != 0 && page >= max_pages {
            break;
        }
        let rows = client.get_episode_list(novel, sort, page).await?;
        if rows.is_empty() {
            break;
        }
        let episode_nos: Vec<u64> = rows.iter().map(|e| e.episode_no).collect();
        if prev_episode_nos.as_ref() == Some(&episode_nos) {
            break;
        }
        all.extend(rows);
        prev_episode_nos = Some(episode_nos);
        page += 1;
    }
    Ok(all)
}

/// CSV row for `novel_<N>_episodes.csv` — episode metadata joined with its
/// real view count (`EpisodeListRow.count_view` is always empty from the
/// list endpoint; the actual value comes from `get_episode_view_counts`).
#[derive(Serialize)]
struct EpisodeExportRow {
    episode_no: u64,
    title: String,
    is_free: bool,
    reg_date: Option<String>,
    count_view: Option<u64>,
}

async fn run_export(
    client: &novelpia::Client,
    novel: u64,
    out: &Path,
    sort: &str,
    no_views: bool,
    max_pages: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out)?;

    // 1. Episode list (paginated).
    let episodes = fetch_all_episodes(client, novel, sort, max_pages).await?;

    // 2. Per-episode view counts (batched over the episode numbers we just found),
    //    joined onto the episode rows by episode_no.
    let mut view_rows = 0usize;
    let mut view_counts: std::collections::HashMap<u64, Option<u64>> =
        std::collections::HashMap::new();
    if !no_views && !episodes.is_empty() {
        let episode_nos: Vec<u64> = episodes.iter().map(|e| e.episode_no).collect();
        let views = client
            .get_episode_view_counts(novel, &episode_nos, false)
            .await?;
        view_rows = views.len();
        view_counts = views
            .iter()
            .map(|v| (v.episode_no, v.count_view_int()))
            .collect();
    }

    let export_rows: Vec<EpisodeExportRow> = episodes
        .iter()
        .map(|e| EpisodeExportRow {
            episode_no: e.episode_no,
            title: e.title.clone(),
            is_free: e.is_free,
            reg_date: e.reg_date.clone(),
            count_view: view_counts.get(&e.episode_no).copied().flatten(),
        })
        .collect();
    let episodes_file = format!("novel_{novel}_episodes.csv");
    write_csv_file(&out.join(&episodes_file), &export_rows)?;
    eprintln!("wrote {} ({} episodes)", episodes_file, export_rows.len());
    let mut files = vec![episodes_file];

    // 3. Reviews.
    let reviews = client.get_novel_review_list(novel).await?;
    let reviews_file = format!("novel_{novel}_reviews.csv");
    write_csv_file(&out.join(&reviews_file), &reviews)?;
    eprintln!("wrote {} ({} reviews)", reviews_file, reviews.len());
    files.push(reviews_file);

    // 4. Manifest.
    let manifest = ExportManifest {
        novel_no: novel,
        exported_at: iso8601_now(),
        sort: sort.to_string(),
        episode_count: episodes.len(),
        view_count_rows: view_rows,
        review_count: reviews.len(),
        files,
    };
    let manifest_path = out.join(format!("novel_{novel}_manifest.json"));
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    eprintln!("wrote {}", manifest_path.display());

    Ok(())
}

/// UTC timestamp in a plain `YYYY-MM-DDTHH:MM:SSZ` form, derived from the
/// system clock without pulling in a datetime crate.
fn iso8601_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso8601(secs)
}

/// Format seconds-since-Unix-epoch as `YYYY-MM-DDTHH:MM:SSZ` (UTC).
///
/// The civil date is computed via Howard Hinnant's `days_from_civil` inverse.
fn format_iso8601(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

/// Reduce a value to the scalar text that `member --scalar` should print,
/// discarding nested arrays and objects (e.g. `badge` lists).
///
/// - A primitive becomes its bare text.
/// - An object is reduced to its scalar-valued fields; a single such field
///   yields its bare value, multiple yield `key=value` lines.
/// - Returns `None` when there is no scalar to print (empty object / array).
fn format_scalar(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null => Some("null".to_string()),
        serde_json::Value::Object(map) => {
            let scalars: Vec<(&String, &serde_json::Value)> = map
                .iter()
                .filter(|(_, v)| {
                    !matches!(
                        v,
                        serde_json::Value::Array(_) | serde_json::Value::Object(_)
                    )
                })
                .collect();
            match scalars.as_slice() {
                [] => None,
                [(_, v)] => format_scalar(v),
                many => Some(
                    many.iter()
                        .filter_map(|(k, v)| format_scalar(v).map(|s| format!("{}={}", k, s)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            }
        }
        serde_json::Value::Array(_) => None,
    }
}

/// Walk a JSON value, collecting every object whose `field` key has a string
/// value matching `re`.  Descends into arrays and nested objects.
fn grep_field_items(
    val: &serde_json::Value,
    field: &str,
    re: &regex::Regex,
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    collect_field_matches(val, field, re, &mut out);
    out
}

fn collect_field_matches(
    val: &serde_json::Value,
    field: &str,
    re: &regex::Regex,
    out: &mut Vec<serde_json::Value>,
) {
    match val {
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_field_matches(item, field, re, out);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(s)) = map.get(field) {
                if re.is_match(s) {
                    out.push(val.clone());
                    return;
                }
            }
            for v in map.values() {
                collect_field_matches(v, field, re, out);
            }
        }
        _ => {}
    }
}

fn print_output<T: Serialize>(
    items: &T,
    fmt: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    match fmt {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(items)?);
        }
        OutputFormat::Csv => {
            let val = serde_json::to_value(items)?;
            if !write_csv(std::io::stdout(), &val)? {
                eprintln!("note: non-array value, falling back to JSON");
                println!("{}", serde_json::to_string_pretty(&val)?);
            }
        }
    }
    Ok(())
}

/// Serialize `items` as CSV to `path`. Returns an error if the value is not a
/// JSON array of objects (export data is always tabular).
fn write_csv_file<T: Serialize>(path: &Path, items: &T) -> Result<(), Box<dyn std::error::Error>> {
    let val = serde_json::to_value(items)?;
    let file = std::fs::File::create(path)?;
    if !write_csv(file, &val)? {
        return Err(format!("cannot write {}: value is not a table", path.display()).into());
    }
    Ok(())
}

/// Write a JSON array-of-objects `val` as CSV to `sink`. The column order
/// follows the first object's keys. Returns `Ok(false)` (writing nothing) when
/// `val` is not an array, so callers can fall back to another format.
fn write_csv<W: std::io::Write>(
    sink: W,
    val: &serde_json::Value,
) -> Result<bool, Box<dyn std::error::Error>> {
    let arr = match val {
        serde_json::Value::Array(arr) => arr,
        _ => return Ok(false),
    };
    let mut wtr = csv::Writer::from_writer(sink);
    let mut header_written = false;
    for item in arr {
        if let serde_json::Value::Object(map) = item {
            if !header_written {
                let keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
                wtr.write_record(&keys)?;
                header_written = true;
            }
            let vals: Vec<String> = map
                .values()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                })
                .collect();
            wtr.write_record(&vals)?;
        }
    }
    wtr.flush()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{EpisodeExportRow, fetch_all_episodes, format_iso8601, format_scalar, write_csv};
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Starts a local HTTP server that answers every request with one HTML
    /// fragment from `pages`, cycling to the last entry once exhausted — this
    /// mimics a server that clamps an out-of-range `page` to the last valid
    /// one instead of returning an empty result. Returns the `http://host:port`
    /// base URL to point a `novelpia::Client` at.
    async fn spawn_episode_list_server(pages: Vec<&'static str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let call_count = Arc::new(AtomicUsize::new(0));

        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let pages = pages.clone();
                let call_count = Arc::clone(&call_count);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    // We don't need to parse the request; each connection is
                    // one request and we only care about call order.
                    let _ = socket.read(&mut buf).await;
                    let idx = call_count.fetch_add(1, Ordering::SeqCst);
                    let body = pages[idx.min(pages.len() - 1)];
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        format!("http://{addr}")
    }

    fn episode_page(episode_no: u64) -> &'static str {
        // Leaked so the fragment can outlive the connection task; test-only.
        Box::leak(
            format!(
                r#"<ul><li class="ep_style" data-episode-no="{episode_no}">
                    <b class="ep_title">ep</b>
                </li></ul>"#
            )
            .into_boxed_str(),
        )
    }

    #[tokio::test]
    async fn fetch_all_episodes_stops_on_repeated_page() {
        // Pages 0 and 1 are distinct; the server then keeps re-serving page 1
        // for any further page number, as a clamping server would.
        let pages = vec![episode_page(1), episode_page(2)];
        let base_url = spawn_episode_list_server(pages).await;
        let client = novelpia::Client::builder()
            .base_url(base_url)
            .delay_ms(0, 0)
            .max_retries(0)
            .build();

        let episodes = fetch_all_episodes(&client, 1, "DOWN", 0).await.unwrap();

        // Only the two genuinely distinct pages are kept; the repeated third
        // page must not be appended again nor cause an infinite loop.
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].episode_no, 1);
        assert_eq!(episodes[1].episode_no, 2);
    }

    #[tokio::test]
    async fn fetch_all_episodes_stops_at_max_pages_cap() {
        // Every page is distinct, so only the hard cap can stop the loop.
        let pages: Vec<&'static str> = (0..10).map(|i| episode_page(i + 1)).collect();
        let base_url = spawn_episode_list_server(pages).await;
        let client = novelpia::Client::builder()
            .base_url(base_url)
            .delay_ms(0, 0)
            .max_retries(0)
            .build();

        let episodes = fetch_all_episodes(&client, 1, "DOWN", 3).await.unwrap();

        assert_eq!(episodes.len(), 3);
    }

    #[tokio::test]
    async fn fetch_all_episodes_stops_on_empty_page() {
        let pages = vec![episode_page(1), "<ul></ul>"];
        let base_url = spawn_episode_list_server(pages).await;
        let client = novelpia::Client::builder()
            .base_url(base_url)
            .delay_ms(0, 0)
            .max_retries(0)
            .build();

        let episodes = fetch_all_episodes(&client, 1, "DOWN", 0).await.unwrap();

        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].episode_no, 1);
    }

    #[test]
    fn iso8601_known_epochs() {
        // Unix epoch.
        assert_eq!(format_iso8601(0), "1970-01-01T00:00:00Z");
        // 2001-09-09T01:46:40Z — the classic 1e9 timestamp.
        assert_eq!(format_iso8601(1_000_000_000), "2001-09-09T01:46:40Z");
        // A leap-year date: 2020-02-29T12:00:00Z.
        assert_eq!(format_iso8601(1_582_977_600), "2020-02-29T12:00:00Z");
    }

    #[test]
    fn csv_writes_array_of_objects() {
        let val = json!([
            { "episode_no": 1, "title": "a", "is_free": true },
            { "episode_no": 2, "title": "b", "is_free": false }
        ]);
        let mut buf = Vec::new();
        let wrote = write_csv(&mut buf, &val).unwrap();
        assert!(wrote);
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        // Header carries every field; column order follows serde_json's map order.
        let header = lines[0];
        assert!(header.contains("episode_no"));
        assert!(header.contains("title"));
        assert!(header.contains("is_free"));
        assert_eq!(lines.len(), 3); // header + 2 rows
        assert!(lines[1].contains('a') && lines[1].contains("true"));
        assert!(lines[2].contains('b') && lines[2].contains("false"));
    }

    #[test]
    fn csv_rejects_non_array() {
        let val = json!({ "not": "a table" });
        let mut buf = Vec::new();
        let wrote = write_csv(&mut buf, &val).unwrap();
        assert!(!wrote);
        assert!(buf.is_empty());
    }

    #[test]
    fn scalar_primitive() {
        assert_eq!(format_scalar(&json!(713)), Some("713".to_string()));
        assert_eq!(format_scalar(&json!("hi")), Some("hi".to_string()));
    }

    #[test]
    fn scalar_object_ignores_arrays() {
        // Real get_member_keep_novel result: badge list is discarded,
        // leaving only the keep_novel count.
        let result = json!({
            "badge": [
                { "badge_url": "/img/new/icon/episode1.svg", "cnt": 713, "is_use": 1, "limit": 1 },
                { "badge_url": "/img/new/icon/episode2.svg", "cnt": 713, "is_use": 1, "limit": 100 }
            ],
            "keep_novel": 713
        });
        assert_eq!(format_scalar(&result), Some("713".to_string()));
    }

    #[test]
    fn scalar_object_multiple_fields() {
        let result = json!({ "read": 100, "comment": 5, "list": [1, 2] });
        let out = format_scalar(&result).unwrap();
        // key=value lines, arrays skipped; order follows serde_json map order.
        assert!(out.contains("read=100"));
        assert!(out.contains("comment=5"));
        assert!(!out.contains("list"));
    }

    #[test]
    fn scalar_no_value() {
        assert_eq!(format_scalar(&json!({ "badge": [1, 2] })), None);
        assert_eq!(format_scalar(&json!([1, 2, 3])), None);
    }

    #[test]
    fn episode_export_row_carries_normalized_count_view() {
        // The joined export row must carry the real, comma-stripped view
        // count (from get_episode_view_counts) rather than the always-empty
        // count_view the episode-list endpoint returns.
        let rows = [
            EpisodeExportRow {
                episode_no: 1134,
                title: "ep1".into(),
                is_free: true,
                reg_date: Some("23.03.23".into()),
                count_view: Some(172_224),
            },
            EpisodeExportRow {
                episode_no: 1135,
                title: "ep2".into(),
                is_free: false,
                reg_date: Some("23.08.24".into()),
                count_view: Some(132_306),
            },
        ];

        // The CSV cell itself must be a plain, unquoted number rather than
        // the raw comma-formatted string — this is the export-facing bug we're
        // guarding against.
        let val = serde_json::to_value(&rows).unwrap();
        let mut buf = Vec::new();
        write_csv(&mut buf, &val).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "count_view,episode_no,is_free,reg_date,title");
        assert_eq!(lines[1], "172224,1134,true,23.03.23,ep1");
        assert_eq!(lines[2], "132306,1135,false,23.08.24,ep2");
    }
}
