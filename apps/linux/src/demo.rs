//! Sample data for `spentday --demo`, so the window can be designed and screenshotted without a
//! database.

use crate::range::{DAY_MS, HOUR_MS};
use spentday_core::{AppUsage, DeviceInfo, DeviceUsage, DomainUsage, Platform, Summary, SummaryQuery, SyncConfig};
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Demo {
    pub devices: Vec<DeviceInfo>,
    pub sync: Mutex<Option<SyncConfig>>,
}

/// Share of an hour in use, by hour of day.
const DAY_SHAPE: [f64; 24] = [
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.12, 0.45, 0.82, 0.93, 0.88, 0.41, 0.63, 0.97, 0.9, 0.77, 0.52, 0.18, 0.32,
    0.58, 0.44, 0.15, 0.0,
];
const WEEK_HOURS: [f64; 7] = [5.4, 6.2, 4.1, 7.3, 5.8, 2.2, 3.1];
const DEVICE_SHARE: [f64; 3] = [0.58, 0.27, 0.15];

/// Domains and each one's share of the app's time.
type Sites = &'static [(&'static str, f64)];

const APPS: &[(&str, &str, f64, Sites)] = &[
    ("firefox", "Firefox", 0.31, &[("github.com", 0.36), ("docs.rs", 0.21), ("youtube.com", 0.17), ("news.ycombinator.com", 0.11), ("mail.proton.me", 0.08), ("", 0.07)]),
    ("code", "Visual Studio Code", 0.22, &[]),
    ("org.gnome.Ptyxis", "Ptyxis", 0.13, &[]),
    ("chrome", "Chrome", 0.11, &[("youtube.com", 0.52), ("docs.google.com", 0.31), ("maps.google.com", 0.17)]),
    ("slack", "Slack", 0.09, &[]),
    ("org.gnome.Nautilus", "Files", 0.05, &[]),
    ("spotify", "Spotify", 0.04, &[]),
    ("org.gnome.TextEditor", "Text Editor", 0.03, &[]),
    ("org.gnome.Calendar", "Calendar", 0.02, &[]),
];

impl Demo {
    pub fn new() -> Demo {
        let device = |id: &str, name: &str, platform| DeviceInfo { id: id.into(), name: name.into(), platform, last_seen_ms: 0 };
        Demo {
            devices: vec![
                device("demo-linux", "thinkpad", Platform::Linux),
                device("demo-mac", "MacBook Air", Platform::Macos),
                device("demo-android", "Pixel 9", Platform::Android),
            ],
            sync: Mutex::new(None),
        }
    }

    pub fn summary(&self, q: SummaryQuery) -> Summary {
        let now = crate::tracker::now_ms();
        let share = match &q.device_id {
            None => 1.0,
            Some(id) => self.devices.iter().position(|d| &d.id == id).map_or(0.0, |i| DEVICE_SHARE[i]),
        };
        let slots = ((q.end_ms - q.start_ms + q.bucket_ms - 1) / q.bucket_ms) as usize;
        let buckets: Vec<i64> = (0..slots)
            .map(|i| {
                let start = q.start_ms + i as i64 * q.bucket_ms;
                let ms = if q.bucket_ms >= DAY_MS {
                    WEEK_HOURS[i % 7] * HOUR_MS as f64
                } else {
                    DAY_SHAPE[i % 24] * HOUR_MS as f64
                };
                let elapsed = ((now - start) as f64 / q.bucket_ms as f64).clamp(0.0, 1.0);
                (ms * share * elapsed) as i64
            })
            .collect();
        let total_ms: i64 = buckets.iter().sum();
        let part = |ratio: f64| (total_ms as f64 * ratio) as i64;
        let apps: Vec<AppUsage> = APPS
            .iter()
            .map(|(key, name, ratio, domains)| AppUsage {
                app_key: key.to_string(),
                app_name: name.to_string(),
                is_browser: !domains.is_empty(),
                total_ms: part(*ratio),
                domains: domains.iter().map(|(d, r)| DomainUsage { domain: d.to_string(), total_ms: part(ratio * r) }).collect(),
                app_ids: vec![key.to_string()],
            })
            .collect();
        let mut sites: HashMap<String, i64> = HashMap::new();
        for domain in apps.iter().flat_map(|a| &a.domains).filter(|d| !d.domain.is_empty()) {
            *sites.entry(domain.domain.clone()).or_default() += domain.total_ms;
        }
        let mut sites: Vec<DomainUsage> = sites.into_iter().map(|(domain, total_ms)| DomainUsage { domain, total_ms }).collect();
        sites.sort_by_key(|s| std::cmp::Reverse(s.total_ms));
        let by_device = self
            .devices
            .iter()
            .zip(DEVICE_SHARE)
            .filter(|(d, _)| q.device_id.as_ref().is_none_or(|id| id == &d.id))
            .map(|(d, s)| DeviceUsage { device: d.clone(), total_ms: (total_ms as f64 * s / share) as i64 })
            .collect();
        Summary { start_ms: q.start_ms, end_ms: q.end_ms, total_ms, by_device, apps, sites, buckets }
    }
}
