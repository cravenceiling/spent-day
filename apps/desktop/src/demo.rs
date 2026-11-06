use crate::{
    colors::{self, Lch, Rgb},
    day,
    layout::union_sessions,
};
use jiff::{Timestamp, civil::date, tz::TimeZone};
use spentday_core::{
    AppUsage, DayDetail, DayItem, DayQuery, DeviceDay, DeviceInfo, DeviceUsage, DomainUsage,
    ItemKind, Platform, Session, Summary, SummaryQuery, SyncConfig,
};
use std::sync::Mutex;

pub struct Demo {
    pub sync: Mutex<Option<SyncConfig>>,
    tz: TimeZone,
}
const ITEMS: &[(&str, &str, ItemKind, &str, &[&str])] = &[
    (
        "xcode",
        "Xcode",
        ItemKind::App,
        "com.apple.dt.Xcode",
        &["com.apple.dt.Xcode"],
    ),
    ("github", "github.com", ItemKind::Site, "", &[]),
    (
        "terminal",
        "Terminal",
        ItemKind::App,
        "com.apple.Terminal",
        &["com.apple.Terminal"],
    ),
    (
        "mail",
        "Mail",
        ItemKind::App,
        "com.apple.mail",
        &["com.apple.mail"],
    ),
    (
        "slack",
        "Slack",
        ItemKind::App,
        "slack",
        &["com.tinyspeck.slackmacgap"],
    ),
    (
        "messages",
        "Messages",
        ItemKind::App,
        "com.apple.MobileSMS",
        &["com.apple.MobileSMS"],
    ),
    ("instagram", "Instagram", ItemKind::App, "instagram", &[]),
    ("youtube", "YouTube", ItemKind::App, "youtube", &[]),
    ("youtubecom", "youtube.com", ItemKind::Site, "", &[]),
    (
        "chrome",
        "Chrome",
        ItemKind::App,
        "chrome",
        &["com.google.Chrome"],
    ),
    ("hn", "news.ycombinator.com", ItemKind::Site, "", &[]),
    (
        "vscode",
        "Visual Studio Code",
        ItemKind::App,
        "vscode",
        &["com.microsoft.VSCode"],
    ),
];
const BLOCKS: &[(&str, &str, i32, i32)] = &[
    ("demo-phone", "messages", 432, 446),
    ("demo-phone", "instagram", 446, 468),
    ("demo-phone", "chrome", 500, 514),
    ("demo-mac", "mail", 529, 550),
    ("demo-mac", "xcode", 550, 655),
    ("demo-mac", "github", 655, 680),
    ("demo-mac", "xcode", 680, 755),
    ("demo-phone", "instagram", 760, 795),
    ("demo-phone", "messages", 795, 808),
    ("demo-mac", "slack", 810, 830),
    ("demo-mac", "xcode", 830, 940),
    ("demo-mac", "terminal", 940, 955),
    ("demo-mac", "github", 955, 990),
    ("demo-mac", "xcode", 990, 1065),
    ("demo-mac", "slack", 1065, 1085),
    ("demo-phone", "youtube", 1075, 1100),
    ("demo-linux", "hn", 1145, 1180),
    ("demo-linux", "vscode", 1180, 1225),
    ("demo-mac", "youtubecom", 1230, 1270),
    ("demo-phone", "instagram", 1250, 1265),
    ("demo-phone", "youtube", 1300, 1355),
    ("demo-phone", "instagram", 1355, 1378),
    ("demo-phone", "messages", 1400, 1418),
];
impl Demo {
    pub fn new() -> Self {
        Self::with_timezone(TimeZone::system())
    }
    pub fn with_timezone(tz: TimeZone) -> Self {
        Self {
            sync: Mutex::new(None),
            tz,
        }
    }
    pub fn day(&self, q: DayQuery) -> DayDetail {
        let selected = Timestamp::from_millisecond(q.start_ms)
            .unwrap()
            .to_zoned(self.tz.clone())
            .date();
        let midnight = day::query(selected, &self.tz).start_ms;
        let fixed = date(2026, 10, 28);
        let history = (1..=4).any(|week| selected == day::step(fixed, -7 * week));
        let populated = selected == fixed || history;
        let mut devices = Vec::new();
        for (id, name, platform) in [
            ("demo-mac", "MacBook Pro", Platform::Macos),
            ("demo-phone", "Pixel 9", Platform::Android),
            ("demo-linux", "ThinkPad", Platform::Linux),
        ] {
            let mut items = Vec::new();
            if populated {
                for &(key, name, kind, app_key, app_ids) in ITEMS {
                    let mut sessions = Vec::new();
                    for &(_, _, from, to) in BLOCKS.iter().filter(|b| b.0 == id && b.1 == key) {
                        let instant = |minute: i32| {
                            let ms = selected
                                .at((minute / 60) as i8, (minute % 60) as i8, 0, 0)
                                .to_zoned(self.tz.clone())
                                .unwrap()
                                .timestamp()
                                .as_millisecond();
                            if history {
                                midnight + (ms - midnight) * 46_980_000 / 49_800_000
                            } else {
                                ms
                            }
                        };
                        let start_ms = instant(from).max(q.start_ms);
                        let end_ms = instant(to).min(q.end_ms);
                        if end_ms > start_ms {
                            sessions.push(Session { start_ms, end_ms });
                        }
                    }
                    if !sessions.is_empty() {
                        items.push(DayItem {
                            key: format!(
                                "{}:{}",
                                if kind == ItemKind::Site {
                                    "site"
                                } else {
                                    "app"
                                },
                                if kind == ItemKind::Site {
                                    name
                                } else {
                                    app_key
                                }
                            ),
                            name: name.into(),
                            kind,
                            app_key: app_key.into(),
                            app_ids: app_ids.iter().map(|s| s.to_string()).collect(),
                            total_ms: sessions.iter().map(|s| s.end_ms - s.start_ms).sum(),
                            sessions,
                        });
                    }
                }
            }
            items.sort_by(|a, b| b.total_ms.cmp(&a.total_ms).then(a.key.cmp(&b.key)));
            if !items.is_empty() {
                devices.push(DeviceDay {
                    device: DeviceInfo {
                        id: id.into(),
                        name: name.into(),
                        platform,
                        last_seen_ms: q.end_ms,
                    },
                    total_ms: items.iter().map(|i| i.total_ms).sum(),
                    items,
                });
            }
        }
        devices.sort_by(|a, b| {
            b.total_ms
                .cmp(&a.total_ms)
                .then(a.device.id.cmp(&b.device.id))
        });
        let wall_ms = union_sessions(
            devices
                .iter()
                .flat_map(|d| d.items.iter().flat_map(|i| i.sessions.clone())),
        )
        .iter()
        .map(|s| s.end_ms - s.start_ms)
        .sum();
        DayDetail {
            start_ms: q.start_ms,
            end_ms: q.end_ms,
            wall_ms,
            device_ms: devices.iter().map(|d| d.total_ms).sum(),
            devices,
        }
    }
    pub fn summary(&self, q: SummaryQuery) -> Summary {
        let detail = self.day(DayQuery {
            start_ms: q.start_ms,
            end_ms: q.end_ms,
        });
        let devices: Vec<_> = detail
            .devices
            .iter()
            .filter(|d| q.device_id.as_ref().is_none_or(|id| *id == d.device.id))
            .collect();
        let mut apps: Vec<AppUsage> = Vec::new();
        let mut sites = Vec::new();
        for item in devices.iter().flat_map(|d| &d.items) {
            if item.kind == ItemKind::Site {
                sites.push(DomainUsage {
                    domain: item.name.clone(),
                    total_ms: item.total_ms,
                });
            } else if let Some(app) = apps.iter_mut().find(|a| a.app_key == item.app_key) {
                app.total_ms += item.total_ms;
                for id in &item.app_ids {
                    if !app.app_ids.contains(id) {
                        app.app_ids.push(id.clone());
                    }
                }
            } else {
                apps.push(AppUsage {
                    app_key: item.app_key.clone(),
                    app_name: item.name.clone(),
                    is_browser: item.app_key == "chrome",
                    total_ms: item.total_ms,
                    domains: Vec::new(),
                    app_ids: item.app_ids.clone(),
                });
            }
        }
        apps.sort_by(|a, b| b.total_ms.cmp(&a.total_ms).then(a.app_key.cmp(&b.app_key)));
        sites.sort_by(|a, b| b.total_ms.cmp(&a.total_ms).then(a.domain.cmp(&b.domain)));
        Summary {
            start_ms: q.start_ms,
            end_ms: q.end_ms,
            total_ms: devices.iter().map(|d| d.total_ms).sum(),
            by_device: devices
                .iter()
                .map(|d| DeviceUsage {
                    device: d.device.clone(),
                    total_ms: d.total_ms,
                })
                .collect(),
            apps,
            sites,
            buckets: Vec::new(),
        }
    }
}
pub fn color_seed(item: &DayItem) -> Lch {
    // Frozen icon samples make screenshots independent of locally installed apps.
    let hex = match item.name.as_str() {
        "Xcode" => 0x4090e7,
        "Mail" => 0x235d99,
        "Visual Studio Code" => 0x59a9e3,
        "Instagram" => 0xd95a94,
        "Chrome" => 0xd5ab34,
        "news.ycombinator.com" => 0xee763f,
        "YouTube" => 0xde5656,
        "youtube.com" => 0xfda29c,
        "Messages" => 0x1a5bb4,
        "Slack" => 0x814d81,
        "github.com" => 0x333333,
        "Terminal" => 0x404040,
        _ => return colors::hashed(&item.key),
    };
    let seed = Rgb::hex(hex).lch();
    if seed.c < 0.02 {
        Lch {
            l: seed.l,
            c: 0.,
            h: 0.,
        }
    } else {
        seed
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_macos_timestamp_and_only_five_populated_dates() {
        let tz = TimeZone::UTC;
        let demo = Demo::with_timezone(tz.clone());
        let selected = Timestamp::from_millisecond(1_793_145_600_000)
            .unwrap()
            .to_zoned(tz.clone())
            .date();
        let detail = demo.day(day::query(selected, &tz));
        assert_eq!((detail.wall_ms, detail.device_ms), (49_800_000, 51_300_000));
        assert_eq!(
            detail
                .devices
                .iter()
                .map(|d| d.total_ms)
                .collect::<Vec<_>>(),
            [32_460_000, 14_040_000, 4_800_000]
        );
        assert_eq!(detail.devices[0].items[0].total_ms, 21_900_000);
        for offset in -35..=1 {
            let detail = demo.day(day::query(day::step(selected, offset), &tz));
            assert_eq!(
                detail.wall_ms > 0,
                [0, -7, -14, -21, -28].contains(&offset),
                "offset {offset}"
            );
        }
    }

    #[test]
    fn summary_supplies_ranked_panel_apps_and_device_filter() {
        let tz = TimeZone::UTC;
        let demo = Demo::with_timezone(tz.clone());
        let day = day::query(date(2026, 10, 28), &tz);
        let mut query = SummaryQuery {
            start_ms: day.start_ms,
            end_ms: day.end_ms,
            bucket_ms: 3_600_000,
            device_id: None,
        };
        let summary = demo.summary(query.clone());
        assert_eq!(summary.total_ms, 51_300_000);
        assert_eq!(
            summary
                .apps
                .iter()
                .take(3)
                .map(|a| (a.app_name.as_str(), a.total_ms))
                .collect::<Vec<_>>(),
            [
                ("Xcode", 21_900_000),
                ("Instagram", 5_700_000),
                ("YouTube", 4_800_000)
            ]
        );
        query.device_id = Some("demo-linux".into());
        let summary = demo.summary(query);
        assert_eq!(summary.total_ms, 4_800_000);
        assert_eq!(summary.by_device.len(), 1);
        assert_eq!(summary.apps[0].app_name, "Visual Studio Code");
        assert_eq!(summary.sites[0].domain, "news.ycombinator.com");
    }

    #[test]
    fn exact_macos_demo() {
        let tz = TimeZone::get("America/New_York").unwrap();
        let demo = Demo::with_timezone(tz.clone());
        let date = date(2026, 10, 28);
        let detail = demo.day(day::query(date, &tz));
        assert_eq!((detail.wall_ms, detail.device_ms), (49_800_000, 51_300_000));
        assert_eq!(
            detail
                .devices
                .iter()
                .map(|d| d.total_ms)
                .collect::<Vec<_>>(),
            vec![32_460_000, 14_040_000, 4_800_000]
        );
        assert_eq!(detail.devices[0].items[0].total_ms, 21_900_000);
        assert_eq!(
            detail
                .devices
                .iter()
                .flat_map(|d| &d.items)
                .flat_map(|i| &i.sessions)
                .count(),
            23
        );
        assert_eq!(demo.day(day::query(day::step(date, 1), &tz)).wall_ms, 0);
        let history: Vec<_> = day::usual_queries(
            date,
            day::step(date, 1).to_zoned(tz.clone()).unwrap().timestamp(),
            &tz,
        )
        .into_iter()
        .map(|q| demo.day(q))
        .collect();
        assert!((day::usual_total(&history).unwrap() - 46_980_000).abs() < 10);
        let partial = demo.day(DayQuery {
            start_ms: detail.start_ms + 550 * 60_000,
            end_ms: detail.start_ms + 600 * 60_000,
        });
        assert_eq!(partial.wall_ms, 50 * 60_000);
    }
}
