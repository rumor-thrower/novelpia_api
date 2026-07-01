//! `npia` — command-line interface for the unofficial Novelpia API client.

use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "npia",
    about = "Unofficial Novelpia API client CLI",
    version
)]
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

        /// Filter array items whose `badge_memo` field matches this regex (e.g. "펀딩|후원")
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

    let mut builder = novelpia::ClientBuilder::default()
        .delay_ms(cli.delay_min, cli.delay_max);
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

        Command::Views { novel, episodes, legacy } => {
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

        Command::Member { mem_no, mode, scalar, grep_memo } => {
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
            if let Some(pattern) = grep_memo {
                let re = regex::Regex::new(pattern)
                    .map_err(|e| format!("invalid --grep-memo pattern: {e}"))?;
                let filtered = grep_memo_items(&val, &re);
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

        Command::Curation { mem_no, novel, page } => {
            let novels = client.get_writer_other_novels(*mem_no, *novel, *page).await?;
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
    }
    Ok(())
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
                .filter(|(_, v)| !matches!(v, serde_json::Value::Array(_) | serde_json::Value::Object(_)))
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

/// Walk a JSON value, collecting every object that has a `memo` field whose
/// string value matches `re`.  Descends into arrays and nested objects.
fn grep_memo_items(val: &serde_json::Value, re: &regex::Regex) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    collect_memo_matches(val, re, &mut out);
    out
}

fn collect_memo_matches(
    val: &serde_json::Value,
    re: &regex::Regex,
    out: &mut Vec<serde_json::Value>,
) {
    match val {
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_memo_matches(item, re, out);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(memo)) = map.get("badge_memo").or_else(|| map.get("memo")) {
                if re.is_match(memo) {
                    out.push(val.clone());
                    return;
                }
            }
            for v in map.values() {
                collect_memo_matches(v, re, out);
            }
        }
        _ => {}
    }
}

fn print_output<T: Serialize>(items: &T, fmt: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    match fmt {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(items)?);
        }
        OutputFormat::Csv => {
            let mut wtr = csv::Writer::from_writer(std::io::stdout());
            // Serialise via JSON value for generic CSV output.
            let val = serde_json::to_value(items)?;
            match val {
                serde_json::Value::Array(arr) => {
                    let mut header_written = false;
                    for item in arr {
                        if let serde_json::Value::Object(map) = item {
                            if !header_written {
                                let keys: Vec<&str> =
                                    map.keys().map(|k| k.as_str()).collect();
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
                }
                other => {
                    eprintln!("note: non-array value, falling back to JSON");
                    println!("{}", serde_json::to_string_pretty(&other)?);
                }
            }
            wtr.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_scalar;
    use serde_json::json;

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
}
