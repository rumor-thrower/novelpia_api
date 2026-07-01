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

        Command::Member { mem_no, mode } => {
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
            println!("{}", serde_json::to_string_pretty(&val)?);
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
