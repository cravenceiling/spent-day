use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chrono::{Days, Local, NaiveTime, TimeZone};
use clap::{Args, Parser, Subcommand, ValueEnum};
use spentday_core::{Activity, SpentDay, Platform, SummaryQuery, SyncConfig, WebPage};

/// Inspect and drive a Spent Day database from the command line.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Time totals for a range (today by default).
    Summary {
        #[arg(long)]
        db: String,
        /// Local midnight to midnight.
        #[arg(long, conflicts_with_all = ["from", "to"])]
        today: bool,
        #[arg(long, requires = "to")]
        from: Option<i64>,
        #[arg(long, requires = "from")]
        to: Option<i64>,
        #[arg(long, default_value_t = 3_600_000)]
        bucket_ms: i64,
        #[arg(long)]
        device: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Raw span rows from every device, newest first.
    Spans {
        #[arg(long)]
        db: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Every known device, this one first.
    Devices {
        #[arg(long)]
        db: String,
        #[arg(long)]
        json: bool,
    },
    /// Write activity as a fake device would. Creates the database when --name and
    /// --platform are given.
    Simulate {
        #[arg(long)]
        db: String,
        #[arg(long, requires = "platform")]
        name: Option<String>,
        #[arg(long, requires = "name")]
        platform: Option<PlatformArg>,
        #[command(subcommand)]
        event: Event,
    },
    /// Sync now. With --server, first stores that server and a sync code read
    /// from SPENTDAY_SYNC_CODE, or from the first line of stdin when it is unset.
    /// The code never goes on the command line, where other users can see it.
    Sync {
        #[arg(long)]
        db: String,
        #[arg(long)]
        server: Option<String>,
    },
    /// Print a fresh sync code.
    NewCode,
}

#[derive(Subcommand)]
enum Event {
    /// One heartbeat (macOS/Linux path).
    Heartbeat {
        #[arg(long)]
        at: Option<i64>,
        #[command(flatten)]
        app: AppArgs,
    },
    /// The user went away: closes the open span.
    Away {
        #[arg(long)]
        at: Option<i64>,
    },
    /// Closes the open span, as on sleep or quit.
    Flush {
        #[arg(long)]
        at: Option<i64>,
    },
    /// Heartbeats every --every ms from --start to --end, then a flush at --end.
    Session {
        #[arg(long)]
        start: i64,
        #[arg(long)]
        end: i64,
        #[arg(long, default_value_t = 5_000)]
        every: i64,
        #[command(flatten)]
        app: AppArgs,
    },
    /// One foreground interval (Android UsageStats path).
    Interval {
        #[arg(long)]
        start: i64,
        #[arg(long)]
        end: i64,
        #[command(flatten)]
        app: AppArgs,
    },
    /// An active-tab report, as the extension or the Android accessibility service sends.
    Hint {
        #[arg(long)]
        at: Option<i64>,
        #[arg(long)]
        browser: String,
        /// Only its domain is stored.
        #[arg(long)]
        url: String,
    },
}

#[derive(Args)]
struct AppArgs {
    #[arg(long)]
    app_id: String,
    /// Defaults to the app id.
    #[arg(long)]
    app_name: Option<String>,
    /// Page URL read by the collector itself. Only its domain is stored.
    /// Without it, browsers take web hints.
    #[arg(long)]
    url: Option<String>,
}

impl AppArgs {
    fn activity(self) -> Activity {
        Activity {
            app_name: self.app_name.unwrap_or_else(|| self.app_id.clone()),
            app_id: self.app_id,
            web: self.url.map(|url| WebPage { url }),
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum PlatformArg {
    Macos,
    Linux,
    Android,
    Ios,
    Windows,
}

impl From<PlatformArg> for Platform {
    fn from(p: PlatformArg) -> Self {
        match p {
            PlatformArg::Macos => Platform::Macos,
            PlatformArg::Linux => Platform::Linux,
            PlatformArg::Android => Platform::Android,
            PlatformArg::Ios => Platform::Ios,
            PlatformArg::Windows => Platform::Windows,
        }
    }
}

fn now_ms() -> i64 {
    Local::now().timestamp_millis()
}

fn today() -> (i64, i64) {
    let midnight = |date: chrono::NaiveDate| {
        Local
            .from_local_datetime(&date.and_time(NaiveTime::MIN))
            .earliest()
            .map_or(0, |t| t.timestamp_millis())
    };
    let date = Local::now().date_naive();
    (midnight(date), midnight(date + Days::new(1)))
}

/// `SPENTDAY_SYNC_CODE`, or the first line of stdin when it is unset.
fn sync_code() -> Result<String> {
    if let Ok(code) = std::env::var("SPENTDAY_SYNC_CODE") {
        return Ok(code);
    }
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("reading the sync code from stdin")?;
    if line.trim().is_empty() {
        bail!("--server needs a sync code in SPENTDAY_SYNC_CODE or on stdin");
    }
    Ok(line.trim().to_owned())
}

fn existing(db: &str) -> Result<Arc<SpentDay>> {
    SpentDay::open_existing(db).with_context(|| format!("opening {db}"))
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn fmt_ms(ms: i64) -> String {
    let s = ms / 1000;
    format!("{}h{:02}m{:02}s", s / 3600, s / 60 % 60, s % 60)
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Summary {
            db,
            today: _,
            from,
            to,
            bucket_ms,
            device,
            json,
        } => {
            let (start_ms, end_ms) = match (from, to) {
                (Some(f), Some(t)) => (f, t),
                _ => today(),
            };
            let summary = existing(&db)?.summary(SummaryQuery {
                start_ms,
                end_ms,
                bucket_ms,
                device_id: device,
            })?;
            if json {
                return print_json(&summary);
            }
            println!("total {}", fmt_ms(summary.total_ms));
            for d in &summary.by_device {
                println!(
                    "device {:<24} {:>10}  {}",
                    d.device.name,
                    fmt_ms(d.total_ms),
                    d.device.id
                );
            }
            for app in &summary.apps {
                println!(
                    "app    {:<24} {:>10}  {}",
                    app.app_name,
                    fmt_ms(app.total_ms),
                    app.app_key
                );
                for d in &app.domains {
                    let domain = if d.domain.is_empty() {
                        "(other)"
                    } else {
                        &d.domain
                    };
                    println!("         {:<22} {:>10}", domain, fmt_ms(d.total_ms));
                }
            }
        }
        Command::Spans { db, limit, json } => {
            let spans = existing(&db)?.recent_spans(limit)?;
            if json {
                return print_json(&spans);
            }
            for s in spans {
                println!(
                    "{} {:>13} {:>13} {:>9}  {}  {}",
                    &s.device_id[..8.min(s.device_id.len())],
                    s.start_ms,
                    s.end_ms,
                    fmt_ms(s.end_ms - s.start_ms),
                    s.app_id,
                    s.domain.unwrap_or_default()
                );
            }
        }
        Command::Devices { db, json } => {
            let devices = existing(&db)?.devices()?;
            if json {
                return print_json(&devices);
            }
            for d in devices {
                println!(
                    "{}  {:<8} {:<24} last seen {}",
                    d.id,
                    format!("{:?}", d.platform).to_lowercase(),
                    d.name,
                    d.last_seen_ms
                );
            }
        }
        Command::Simulate {
            db,
            name,
            platform,
            event,
        } => {
            let m = match (name, platform) {
                (Some(name), Some(platform)) => SpentDay::open(db, name, platform.into())?,
                _ => existing(&db).context("pass --name and --platform to create a device")?,
            };
            match event {
                Event::Heartbeat { at, app } => {
                    m.heartbeat(at.unwrap_or_else(now_ms), Some(app.activity()))?
                }
                Event::Away { at } => m.heartbeat(at.unwrap_or_else(now_ms), None)?,
                Event::Flush { at } => m.flush(at.unwrap_or_else(now_ms))?,
                Event::Session {
                    start,
                    end,
                    every,
                    app,
                } => {
                    if every <= 0 || end < start {
                        bail!("need --every > 0 and --end >= --start");
                    }
                    let activity = app.activity();
                    let mut at = start;
                    while at < end {
                        m.heartbeat(at, Some(activity.clone()))?;
                        at += every;
                    }
                    m.heartbeat(end, Some(activity))?;
                    m.flush(end)?;
                }
                Event::Interval { start, end, app } => {
                    m.record_interval(start, end, app.activity())?
                }
                Event::Hint { at, browser, url } => {
                    m.web_hint(at.unwrap_or_else(now_ms), browser, WebPage { url })?
                }
            }
        }
        Command::Sync { db, server } => {
            let m = existing(&db)?;
            if let Some(server_url) = server {
                m.set_sync_config(Some(SyncConfig {
                    server_url,
                    sync_code: sync_code()?,
                }))?;
            }
            print_json(&m.sync_now()?)?;
        }
        Command::NewCode => {
            let scratch = SpentDay::open(":memory:".into(), "spentday-cli".into(), Platform::Linux)?;
            println!("{}", scratch.new_sync_code());
        }
    }
    Ok(())
}
