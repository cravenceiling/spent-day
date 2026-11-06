//! Summary sentences and the three overview detail lines.

use crate::{day, format::duration, layout::union_sessions};
use jiff::{Timestamp, tz::TimeZone};
use spentday_core::{DayDetail, DayItem, DeviceDay};

pub fn day_sentence(
    detail: &DayDetail,
    usual_ms: Option<i64>,
    weekday: &str,
    tz: &TimeZone,
) -> String {
    let sessions: Vec<_> = detail
        .devices
        .iter()
        .flat_map(|d| &d.items)
        .flat_map(|i| &i.sessions)
        .collect();
    let (Some(first), Some(last)) = (
        sessions.iter().map(|s| s.start_ms).min(),
        sessions.iter().map(|s| s.end_ms).max(),
    ) else {
        return String::new();
    };
    let comparison = usual_text(detail.wall_ms, usual_ms, weekday)
        .map(|text| format!("{text}. "))
        .unwrap_or_default();
    format!(
        "{comparison}First screen {}, last {}.",
        day::clock(first, detail.end_ms, tz),
        day::clock(last, detail.end_ms, tz)
    )
}

pub fn device_sentence(device: &DeviceDay, day_end: i64, tz: &TimeZone) -> String {
    let sessions: Vec<_> = device.items.iter().flat_map(|i| &i.sessions).collect();
    let (Some(first), Some(last)) = (
        sessions.iter().map(|s| s.start_ms).min(),
        sessions.iter().map(|s| s.end_ms).max(),
    ) else {
        return String::new();
    };
    format!(
        "{}, {} {}, {} to {}.",
        device.device.name,
        device.items.len(),
        if device.items.len() == 1 {
            "app or site"
        } else {
            "apps and sites"
        },
        day::clock(first, day_end, tz),
        day::clock(last, day_end, tz)
    )
}

pub fn item_sentence(item: &DayItem, day_end: i64, tz: &TimeZone) -> String {
    let (Some(first), Some(last)) = (
        item.sessions.iter().map(|s| s.start_ms).min(),
        item.sessions.iter().map(|s| s.end_ms).max(),
    ) else {
        return String::new();
    };
    format!(
        "{}, {} {}, {} to {}.",
        item.name,
        item.sessions.len(),
        if item.sessions.len() == 1 {
            "session"
        } else {
            "sessions"
        },
        day::clock(first, day_end, tz),
        day::clock(last, day_end, tz)
    )
}

/// Comparison without its trailing sentence punctuation.
pub fn usual_text(total_ms: i64, usual_ms: Option<i64>, weekday: &str) -> Option<String> {
    let delta = total_ms - usual_ms?;
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

pub fn details_lines(detail: &DayDetail, tz: &TimeZone) -> Vec<String> {
    let mut lines = vec![format!(
        "Time on two screens at once: {}. {} of device time.",
        duration(detail.device_ms - detail.wall_ms),
        duration(detail.device_ms)
    )];
    let longest = detail
        .devices
        .iter()
        .flat_map(|d| {
            union_sessions(d.items.iter().flat_map(|i| i.sessions.clone()))
                .into_iter()
                .map(move |s| (&d.device.name, s))
        })
        .max_by_key(|(_, s)| s.end_ms - s.start_ms);
    if let Some((name, session)) = longest {
        lines.push(format!(
            "Longest stretch: {} on {name}, {} to {}.",
            duration(session.end_ms - session.start_ms),
            day::clock(session.start_ms, detail.end_ms, tz),
            day::clock(session.end_ms, detail.end_ms, tz)
        ));
    }
    let date = Timestamp::from_millisecond(detail.start_ms)
        .expect("day timestamp")
        .to_zoned(tz.clone())
        .date();
    let late = date
        .at(23, 0, 0, 0)
        .to_zoned(tz.clone())
        .expect("local 23:00")
        .timestamp()
        .as_millisecond();
    let late_ms = union_sessions(
        detail
            .devices
            .iter()
            .flat_map(|d| &d.items)
            .flat_map(|i| i.sessions.clone()),
    )
    .iter()
    .map(|s| (s.end_ms - s.start_ms.max(late)).max(0))
    .sum();
    lines.push(format!("Time after 23:00: {}.", duration(late_ms)));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::Demo;
    use jiff::civil::date;

    #[test]
    fn exact_demo_sentences_and_details() {
        let tz = TimeZone::get("America/New_York").unwrap();
        let detail = Demo::with_timezone(tz.clone()).day(day::query(date(2026, 10, 28), &tz));
        assert_eq!(
            day_sentence(&detail, Some(46_980_000), "Wednesday", &tz),
            "47m more than a usual Wednesday. First screen 07:12, last 23:38."
        );
        assert_eq!(
            device_sentence(&detail.devices[0], detail.end_ms, &tz),
            "MacBook Pro, 6 apps and sites, 08:49 to 21:10."
        );
        assert_eq!(
            item_sentence(&detail.devices[0].items[0], detail.end_ms, &tz),
            "Xcode, 4 sessions, 09:10 to 17:45."
        );
        assert_eq!(
            details_lines(&detail, &tz),
            vec![
                "Time on two screens at once: 25m. 14h 15m of device time.",
                "Longest stretch: 4h 35m on MacBook Pro, 13:30 to 18:05.",
                "Time after 23:00: 18m.",
            ]
        );
    }

    #[test]
    fn comparison_boundaries_and_empty_day() {
        assert_eq!(usual_text(100_000, None, "Monday"), None);
        assert_eq!(
            usual_text(100_000, Some(159_999), "Monday").as_deref(),
            Some("About the same as a usual Monday")
        );
        assert_eq!(
            usual_text(100_000, Some(160_000), "Monday").as_deref(),
            Some("1m less than a usual Monday")
        );
        assert_eq!(
            usual_text(160_000, Some(100_000), "Monday").as_deref(),
            Some("1m more than a usual Monday")
        );
        let tz = TimeZone::UTC;
        let empty = Demo::with_timezone(tz.clone()).day(day::query(date(2026, 10, 29), &tz));
        assert_eq!(day_sentence(&empty, Some(100), "Thursday", &tz), "");
        assert_eq!(
            details_lines(&empty, &tz),
            [
                "Time on two screens at once: 0m. 0m of device time.",
                "Time after 23:00: 0m."
            ]
        );
    }

    #[test]
    fn late_time_uses_local_23_on_both_dst_days() {
        let tz = TimeZone::get("America/New_York").unwrap();
        for date in [date(2026, 3, 8), date(2026, 11, 1)] {
            let mut detail = Demo::with_timezone(tz.clone())
                .day(day::query(jiff::civil::date(2026, 10, 28), &tz));
            let query = day::query(date, &tz);
            detail.start_ms = query.start_ms;
            detail.end_ms = query.end_ms;
            let start = date
                .at(22, 30, 0, 0)
                .to_zoned(tz.clone())
                .unwrap()
                .timestamp()
                .as_millisecond();
            for device in &mut detail.devices {
                device.items.truncate(1);
                device.items[0].sessions = vec![spentday_core::Session {
                    start_ms: start,
                    end_ms: query.end_ms,
                }];
            }
            assert_eq!(
                details_lines(&detail, &tz).last().unwrap(),
                "Time after 23:00: 1h."
            );
        }
    }
}
