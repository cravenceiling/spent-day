use std::collections::BTreeMap;

use crate::{DeviceInfo, Millis, apps, store::Span};

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DayQuery {
    pub start_ms: Millis,
    pub end_ms: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DayDetail {
    pub start_ms: Millis,
    pub end_ms: Millis,
    pub wall_ms: Millis,
    pub device_ms: Millis,
    pub devices: Vec<DeviceDay>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DeviceDay {
    pub device: DeviceInfo,
    pub total_ms: Millis,
    pub items: Vec<DayItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ItemKind {
    App,
    Site,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DayItem {
    pub key: String,
    pub name: String,
    pub kind: ItemKind,
    pub app_key: String,
    pub app_ids: Vec<String>,
    pub total_ms: Millis,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Session {
    pub start_ms: Millis,
    pub end_ms: Millis,
}

pub fn detail(query: &DayQuery, spans: &[Span], devices: &[DeviceInfo]) -> DayDetail {
    let mut result = DayDetail {
        start_ms: query.start_ms,
        end_ms: query.end_ms,
        wall_ms: 0,
        device_ms: 0,
        devices: Vec::new(),
    };
    for device in devices {
        let mut own: Vec<_> = spans
            .iter()
            .filter(|s| {
                s.device_id == device.id
                    && s.end_ms.min(query.end_ms) > s.start_ms.max(query.start_ms)
            })
            .collect();
        own.sort_by(|a, b| (a.start_ms, a.end_ms, &a.id).cmp(&(b.start_ms, b.end_ms, &b.id)));
        let mut items: BTreeMap<String, (DayItem, BTreeMap<String, Millis>, Millis)> =
            BTreeMap::new();
        let mut previous_key = String::new();
        for span in own {
            let start = span.start_ms.max(query.start_ms);
            let end = span.end_ms.min(query.end_ms);
            let app_key = apps::app_key(&span.app_id);
            let site = span.domain.as_ref().filter(|d| !d.is_empty());
            let key = site.map_or_else(|| format!("app:{app_key}"), |d| format!("site:{d}"));
            let (item, ids, name_seen_at) = items.entry(key.clone()).or_insert_with(|| {
                (
                    DayItem {
                        key: key.clone(),
                        name: String::new(),
                        kind: if site.is_some() {
                            ItemKind::Site
                        } else {
                            ItemKind::App
                        },
                        app_key: if site.is_some() {
                            String::new()
                        } else {
                            app_key
                        },
                        app_ids: Vec::new(),
                        total_ms: 0,
                        sessions: Vec::new(),
                    },
                    BTreeMap::new(),
                    0,
                )
            });
            if item.name.is_empty() || span.end_ms > *name_seen_at {
                item.name = site.cloned().unwrap_or_else(|| {
                    apps::lookup(&span.app_id)
                        .map_or_else(|| span.app_name.clone(), |a| a.name.to_owned())
                });
                *name_seen_at = span.end_ms;
            }
            *ids.entry(span.app_id.clone()).or_default() += end - start;
            if let Some(last) = item
                .sessions
                .last_mut()
                .filter(|last| previous_key == key && start.saturating_sub(last.end_ms) <= 60_000)
            {
                last.end_ms = last.end_ms.max(end);
            } else {
                item.sessions.push(Session {
                    start_ms: start,
                    end_ms: end,
                });
            }
            previous_key = key;
        }
        let mut items: Vec<_> = items
            .into_values()
            .map(|(mut item, ids, _)| {
                let mut ids: Vec<_> = ids.into_iter().collect();
                ids.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                item.app_ids = ids.into_iter().map(|(id, _)| id).collect();
                item.total_ms = item.sessions.iter().map(|s| s.end_ms - s.start_ms).sum();
                item
            })
            .collect();
        items.sort_by(|a, b| b.total_ms.cmp(&a.total_ms).then(a.key.cmp(&b.key)));
        let total_ms = items.iter().map(|i| i.total_ms).sum();
        if total_ms > 0 {
            result.devices.push(DeviceDay {
                device: device.clone(),
                total_ms,
                items,
            });
        }
    }
    result.devices.sort_by(|a, b| {
        b.total_ms
            .cmp(&a.total_ms)
            .then(a.device.id.cmp(&b.device.id))
    });
    result.device_ms = result.devices.iter().map(|d| d.total_ms).sum();
    let mut sessions: Vec<_> = result
        .devices
        .iter()
        .flat_map(|d| &d.items)
        .flat_map(|i| &i.sessions)
        .collect();
    sessions.sort_by_key(|s| (s.start_ms, s.end_ms));
    let mut end = query.start_ms;
    for session in sessions {
        result.wall_ms += (session.end_ms - session.start_ms.max(end)).max(0);
        end = end.max(session.end_ms);
    }
    result
}
