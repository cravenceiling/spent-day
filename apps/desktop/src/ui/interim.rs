//! INTERIM, delete when unit 1 merges. Stand-ins for unit 1's `day`, `stats`, `cells::layout`,
//! the demo dataset and the direction B palette maths, so the dashboard renders real scenes
//! before the model lands. Every caller imports from here in one `use` line; at merge, point
//! those at `crate::{day, stats}` / `crate::ui::cells`, delete this file and its `mod` line in
//! `dash.rs`, and replace `treemap::look` with the `Palette` lookup.

use crate::colors::{Lch, Rgb};
use crate::layout::{self, MapItem, Rect};
use crate::ui::treemap::Look;
use jiff::Timestamp;
use jiff::civil::{Date, date};
use jiff::tz::TimeZone;
use spentday_core::{
    DayDetail, DayItem, DayQuery, DeviceDay, DeviceInfo, ItemKind, Platform, Session,
};

pub mod day {
    use super::*;

    pub use crate::day::{today, usual_total};

    fn ms(date: Date, hour: i8, minute: i8, tz: &TimeZone) -> i64 {
        date.at(hour, minute, 0, 0)
            .to_zoned(tz.clone())
            .map_or(0, |z| z.timestamp().as_millisecond())
    }

    pub fn query(date: Date, tz: &TimeZone) -> DayQuery {
        DayQuery {
            start_ms: ms(date, 0, 0, tz),
            end_ms: ms(step(date, 1), 0, 0, tz),
        }
    }

    pub fn step(date: Date, days: i32) -> Date {
        date.checked_add(jiff::Span::new().days(days))
            .unwrap_or(date)
    }

    pub fn usual_queries(date: Date, now: Timestamp, tz: &TimeZone) -> Vec<DayQuery> {
        let now = now.to_zoned(tz.clone());
        (1..=4)
            .map(|week| {
                let previous = step(date, -7 * week);
                let mut q = query(previous, tz);
                if now.date() == date {
                    q.end_ms = ms(previous, now.hour(), now.minute(), tz);
                }
                q
            })
            .collect()
    }

    pub fn clock(ms: i64, day_end: i64, tz: &TimeZone) -> String {
        if ms == day_end {
            return "24:00".into();
        }
        Timestamp::from_millisecond(ms)
            .map(|t| t.to_zoned(tz.clone()).strftime("%H:%M").to_string())
            .unwrap_or_default()
    }

    pub fn accessible_duration(ms: i64) -> String {
        let minutes = (ms.max(0) + 30_000) / 60_000;
        let unit =
            |n: i64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        if minutes < 60 {
            unit(minutes, "minute", "minutes")
        } else {
            format!(
                "{} {}",
                unit(minutes / 60, "hour", "hours"),
                unit(minutes % 60, "minute", "minutes")
            )
        }
    }

    pub fn header_date(date: Date) -> String {
        date.strftime("%A, %-d %B").to_string()
    }

    pub fn weekday_name(date: Date) -> &'static str {
        use jiff::civil::Weekday::*;
        match date.weekday() {
            Monday => "Monday",
            Tuesday => "Tuesday",
            Wednesday => "Wednesday",
            Thursday => "Thursday",
            Friday => "Friday",
            Saturday => "Saturday",
            Sunday => "Sunday",
        }
    }
}

pub mod stats {
    use super::*;
    use crate::format::duration;

    fn span(sessions: impl Iterator<Item = Session>) -> Option<(i64, i64)> {
        let all: Vec<_> = sessions.collect();
        Some((
            all.iter().map(|s| s.start_ms).min()?,
            all.iter().map(|s| s.end_ms).max()?,
        ))
    }

    pub fn day_sentence(d: &DayDetail, usual: Option<i64>, weekday: &str, tz: &TimeZone) -> String {
        let all = d
            .devices
            .iter()
            .flat_map(|v| v.items.iter().flat_map(|i| i.sessions.clone()));
        let Some((first, last)) = span(all) else {
            return String::new();
        };
        let comparison = usual_text(d.wall_ms, usual, weekday)
            .map(|t| format!("{t}. "))
            .unwrap_or_default();
        format!(
            "{comparison}First screen {}, last {}.",
            day::clock(first, d.end_ms, tz),
            day::clock(last, d.end_ms, tz)
        )
    }

    pub fn device_sentence(device: &DeviceDay, day_end: i64, tz: &TimeZone) -> String {
        let all = device.items.iter().flat_map(|i| i.sessions.clone());
        let Some((first, last)) = span(all) else {
            return String::new();
        };
        let n = device.items.len();
        format!(
            "{}, {n} {}, {} to {}.",
            device.device.name,
            if n == 1 {
                "app or site"
            } else {
                "apps and sites"
            },
            day::clock(first, day_end, tz),
            day::clock(last, day_end, tz)
        )
    }

    pub fn item_sentence(item: &DayItem, day_end: i64, tz: &TimeZone) -> String {
        let Some((first, last)) = span(item.sessions.iter().cloned()) else {
            return String::new();
        };
        let n = item.sessions.len();
        format!(
            "{n} {}, {} to {}.",
            if n == 1 { "session" } else { "sessions" },
            day::clock(first, day_end, tz),
            day::clock(last, day_end, tz)
        )
    }

    pub fn usual_text(total: i64, usual: Option<i64>, weekday: &str) -> Option<String> {
        let delta = total - usual?;
        Some(if delta.abs() < 60_000 {
            format!("About the same as a usual {weekday}")
        } else {
            format!(
                "{} {} than a usual {weekday}",
                duration(delta.abs()),
                if delta > 0 { "more" } else { "less" }
            )
        })
    }

    pub fn details_lines(d: &DayDetail, tz: &TimeZone) -> Vec<String> {
        let mut lines = vec![format!(
            "Time on two screens at once: {}. {} of device time.",
            duration(d.device_ms - d.wall_ms),
            duration(d.device_ms)
        )];
        let longest = d
            .devices
            .iter()
            .flat_map(|v| {
                layout::union_sessions(v.items.iter().flat_map(|i| i.sessions.clone()))
                    .into_iter()
                    .map(move |s| (&v.device.name, s))
            })
            .max_by_key(|(_, s)| s.end_ms - s.start_ms);
        if let Some((name, s)) = longest {
            lines.push(format!(
                "Longest stretch: {} on {name}, {} to {}.",
                duration(s.end_ms - s.start_ms),
                day::clock(s.start_ms, d.end_ms, tz),
                day::clock(s.end_ms, d.end_ms, tz)
            ));
        }
        let date = Timestamp::from_millisecond(d.start_ms)
            .map(|t| t.to_zoned(tz.clone()).date())
            .unwrap_or_default();
        let late = date
            .at(23, 0, 0, 0)
            .to_zoned(tz.clone())
            .map_or(d.end_ms, |z| z.timestamp().as_millisecond());
        let late_ms: i64 = layout::union_sessions(
            d.devices
                .iter()
                .flat_map(|v| v.items.iter().flat_map(|i| i.sessions.clone())),
        )
        .iter()
        .map(|s| (s.end_ms - s.start_ms.max(late)).max(0))
        .sum();
        lines.push(format!("Time after 23:00: {}.", duration(late_ms)));
        lines
    }
}

pub mod cells {
    use super::*;
    use crate::palette::Palette;
    use crate::ui::cells::{Cell, CellKind, Header, Label, MapLayout, Measure};
    use crate::ui::dash::Drill;

    fn label(name: &str, duration: &str, w: f32, h: f32, measure: Measure) -> Label {
        let tiers: &[(f32, f32, f32)] = if w >= 260.0 && h >= 200.0 {
            &[(16.0, 22.0, 15.0), (13.0, 15.0, 13.0), (12.0, 13.0, 12.5)]
        } else {
            &[(13.0, 15.0, 13.0), (12.0, 13.0, 12.5)]
        };
        for &(pad, name_pt, dur_pt) in tiers {
            let (nw, _) = measure(name, name_pt, 650.0);
            let (dw, _) = measure(duration, dur_pt, 500.0);
            if w - 2.0 * pad >= nw.max(dw) + 2.0 && h - 2.0 * pad >= (name_pt + dur_pt) * 1.4 + 2.0
            {
                return Label::Both {
                    pad,
                    name_pt,
                    dur_pt,
                };
            }
        }
        let (nw, nh) = measure(name, 13.0, 650.0);
        if w - 24.0 >= nw + 2.0 && h - 20.0 >= nh {
            Label::NameOnly
        } else {
            Label::None
        }
    }

    fn cell(
        m: MapItem,
        index: Option<usize>,
        r: Rect,
        palette: &Palette,
        dark: bool,
        measure: Measure,
    ) -> Cell {
        let _ = (palette, dark);
        let look = super::look(&m.item.key, dark, false);
        let dim = super::look(&m.item.key, dark, true);
        let total = m.total_ms();
        let shown = if index.is_some() {
            "00:00 to 00:00"
        } else {
            m.item.name.as_str()
        };
        let (w, h) = (r.w as f32, r.h as f32);
        let kind = match index {
            Some(index) => CellKind::Session {
                device: m.device_id.clone(),
                key: m.item.key.clone(),
                index,
            },
            None if !m.hidden.is_empty() => CellKind::Other {
                device: m.device_id.clone(),
                hidden: m
                    .hidden
                    .iter()
                    .map(|i| (i.name.clone(), i.total_ms))
                    .collect(),
            },
            None => CellKind::Item {
                device: m.device_id.clone(),
                key: m.item.key.clone(),
            },
        };
        Cell {
            id: m.id.clone(),
            kind,
            rect: r,
            name: m.item.name.clone(),
            total_ms: total,
            sessions: m.sessions().to_vec(),
            fill: look.top,
            dimmed: dim.top,
            ink: look.ink,
            label: label(shown, &crate::format::duration(total), w, h, measure),
            radius: 10f32.min(w / 3.0).min(h / 3.0),
        }
    }

    pub fn layout(
        detail: &DayDetail,
        drill: &Drill,
        palette: &Palette,
        size: [f32; 2],
        dark: bool,
        measure: Measure,
    ) -> MapLayout {
        let bounds = Rect::new(0.0, 0.0, size[0] as f64, size[1] as f64);
        let device_id = match drill {
            Drill::Day => None,
            Drill::Device(d) | Drill::Item { device: d, .. } => Some(d),
        };
        let mut out = MapLayout::default();
        let Some(device) = detail
            .devices
            .iter()
            .find(|d| Some(&d.device.id) == device_id)
        else {
            for g in layout::device_groups(detail, bounds) {
                out.headers.push(Header {
                    device: g.device.device.id.clone(),
                    name: g.device.device.name.clone(),
                    total_ms: g.device.total_ms,
                    rect: g.label,
                });
                for (m, r) in g.tiles {
                    out.cells.push(cell(m, None, r, palette, dark, measure));
                }
            }
            return out;
        };
        let bounds = bounds.inset(-4.0);
        if let Drill::Item { key, .. } = drill
            && let Some(item) = device.items.iter().find(|i| i.key == *key)
        {
            let mut sessions = item.sessions.clone();
            sessions.sort_by_key(|s| s.start_ms);
            let values: Vec<f64> = sessions
                .iter()
                .map(|s| (s.end_ms - s.start_ms) as f64)
                .collect();
            for (index, (session, r)) in sessions
                .into_iter()
                .zip(layout::slice(&values, bounds))
                .enumerate()
            {
                let m = MapItem {
                    id: format!("{}/{}/{index}", device.device.id, item.key),
                    device_id: device.device.id.clone(),
                    item: item.clone(),
                    hidden: vec![],
                    session: Some(session),
                };
                out.cells
                    .push(cell(m, Some(index), r.inset(4.0), palette, dark, measure));
            }
            return out;
        }
        let items = layout::map_items(device, detail.wall_ms);
        let values: Vec<f64> = items.iter().map(|i| i.total_ms() as f64).collect();
        for (m, r) in items.into_iter().zip(layout::squarify(&values, bounds)) {
            out.cells
                .push(cell(m, None, r.inset(4.0), palette, dark, measure));
        }
        out
    }
}

/// Direction B stops from `palette/options.md` for the demo keys; other keys get a deep jewel
/// hue from their hash.
pub fn look(key: &str, dark: bool, dimmed: bool) -> Look {
    let (top, bottom, white) = match key {
        "app:com.apple.dt.Xcode" => (0x1d71e2, 0x2d53c9, true),
        "app:instagram" => (0xd02c92, 0xba005d, true),
        "app:youtube" => (0xde2d27, 0xbe0041, true),
        "site:github.com" => (0x596b9c, 0x4c5484, true),
        "app:com.apple.MobileSMS" => (0x87b8ff, 0x799bee, false),
        "app:vscode" => (0x47ebdc, 0x00d0d9, false),
        "app:slack" => (0xc030ce, 0xb30095, true),
        "site:youtube.com" => (0xff9386, 0xec7282, false),
        "site:news.ycombinator.com" => (0xfdc233, 0xf49f1d, false),
        "app:com.apple.mail" => (0x5b3cf9, 0x5c00d7, true),
        "app:com.apple.Terminal" => (0x2d7a82, 0x126371, true),
        "app:chrome" => (0xace709, 0x5ed740, false),
        "other" => (0xa7b0d6, 0x939dc8, false),
        _ => {
            let anchors = [258.0, 280.0, 304.0, 330.0, 354.0, 22.0, 192.0, 232.0];
            let hash = key
                .bytes()
                .fold(5381u64, |h, b| h.wrapping_mul(33).wrapping_add(b as u64));
            let h = anchors[(hash % anchors.len() as u64) as usize];
            let top = Lch {
                l: 0.585,
                c: 0.2,
                h,
            }
            .rgb();
            let bottom = Lch {
                l: 0.51,
                c: 0.2,
                h: h + 8.0,
            }
            .rgb();
            return finish(top, bottom, Rgb::WHITE, dark, dimmed);
        }
    };
    let ink = if white { Rgb::WHITE } else { Rgb::BLACK };
    finish(Rgb::hex(top), Rgb::hex(bottom), ink, dark, dimmed)
}

fn finish(top: Rgb, bottom: Rgb, ink: Rgb, dark: bool, dimmed: bool) -> Look {
    if !dimmed {
        return Look { top, bottom, ink };
    }
    let sink = |c: Rgb| {
        let Lch { l, c, h } = c.lch();
        if dark {
            Lch {
                l: 0.30 + (l - 0.6) * 0.08,
                c: (c * 0.35).min(0.06),
                h,
            }
            .rgb()
        } else {
            Lch {
                l: 0.92 + (l - 0.6) * 0.05,
                c: (c * 0.30).min(0.045),
                h,
            }
            .rgb()
        }
    };
    Look {
        top: sink(top),
        bottom: sink(bottom),
        ink: if dark { Rgb::WHITE } else { Rgb::BLACK },
    }
}

const ITEMS: &[(&str, &str, ItemKind, &str)] = &[
    ("xcode", "Xcode", ItemKind::App, "com.apple.dt.Xcode"),
    ("github", "github.com", ItemKind::Site, ""),
    ("terminal", "Terminal", ItemKind::App, "com.apple.Terminal"),
    ("mail", "Mail", ItemKind::App, "com.apple.mail"),
    ("slack", "Slack", ItemKind::App, "slack"),
    ("messages", "Messages", ItemKind::App, "com.apple.MobileSMS"),
    ("instagram", "Instagram", ItemKind::App, "instagram"),
    ("youtube", "YouTube", ItemKind::App, "youtube"),
    ("youtubecom", "youtube.com", ItemKind::Site, ""),
    ("chrome", "Chrome", ItemKind::App, "chrome"),
    ("hn", "news.ycombinator.com", ItemKind::Site, ""),
    ("vscode", "Visual Studio Code", ItemKind::App, "vscode"),
];

const BLOCKS: &[(&str, &str, i16, i16)] = &[
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

/// The Linux demo dataset: 2026-10-28 and the four Wednesdays before it (scaled to a 46 980 000
/// ms usual day); every other day is empty.
pub fn demo_day(q: &DayQuery, tz: &TimeZone) -> DayDetail {
    let day = Timestamp::from_millisecond(q.start_ms)
        .map(|t| t.to_zoned(tz.clone()).date())
        .unwrap_or_default();
    let fixed = date(2026, 10, 28);
    let history = (1..=4).any(|w| day == day::step(fixed, -7 * w));
    let midnight = day::query(day, tz).start_ms;
    let mut devices = Vec::new();
    for (id, name, platform) in [
        ("demo-mac", "MacBook Pro", Platform::Macos),
        ("demo-phone", "Pixel 9", Platform::Android),
        ("demo-linux", "ThinkPad", Platform::Linux),
    ] {
        let mut items = Vec::new();
        for &(key, name, kind, app_key) in ITEMS.iter().filter(|_| day == fixed || history) {
            let instant = |minute: i16| {
                let ms = day
                    .at((minute / 60) as i8, (minute % 60) as i8, 0, 0)
                    .to_zoned(tz.clone())
                    .map_or(0, |z| z.timestamp().as_millisecond());
                if history {
                    midnight + (ms - midnight) * 46_980_000 / 49_800_000
                } else {
                    ms
                }
            };
            let sessions: Vec<Session> = BLOCKS
                .iter()
                .filter(|b| b.0 == id && b.1 == key)
                .map(|b| Session {
                    start_ms: instant(b.2).max(q.start_ms),
                    end_ms: instant(b.3).min(q.end_ms),
                })
                .filter(|s| s.end_ms > s.start_ms)
                .collect();
            if sessions.is_empty() {
                continue;
            }
            let site = kind == ItemKind::Site;
            items.push(DayItem {
                key: format!(
                    "{}:{}",
                    if site { "site" } else { "app" },
                    if site { name } else { app_key }
                ),
                name: name.into(),
                kind,
                app_key: app_key.into(),
                app_ids: if site { vec![] } else { vec![app_key.into()] },
                total_ms: sessions.iter().map(|s| s.end_ms - s.start_ms).sum(),
                sessions,
            });
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
    let wall_ms = layout::union_sessions(
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
