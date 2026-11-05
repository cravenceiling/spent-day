use gtk::glib;
use spentday_core::{DayDetail, DayQuery};

pub fn midnight(date: &glib::DateTime) -> glib::DateTime {
    glib::DateTime::new(
        &date.timezone(),
        date.year(),
        date.month(),
        date.day_of_month(),
        0,
        0,
        0.,
    )
    .expect("local midnight")
}
pub fn query(date: &glib::DateTime) -> DayQuery {
    let start = midnight(date);
    DayQuery {
        start_ms: start.to_unix() * 1000,
        end_ms: start.add_days(1).expect("next day").to_unix() * 1000,
    }
}
pub fn usual_queries(date: &glib::DateTime, now: &glib::DateTime) -> Vec<DayQuery> {
    let today = query(date).start_ms == query(now).start_ms;
    (1..=4)
        .map(|week| {
            let previous = date.add_days(-7 * week).expect("previous weekday");
            let mut q = query(&previous);
            if today {
                q.end_ms = glib::DateTime::new(
                    &date.timezone(),
                    previous.year(),
                    previous.month(),
                    previous.day_of_month(),
                    now.hour(),
                    now.minute(),
                    now.seconds(),
                )
                .expect("local time")
                .to_unix()
                    * 1000;
            }
            q
        })
        .collect()
}
pub fn usual_total(days: &[DayDetail]) -> Option<i64> {
    let totals: Vec<_> = days
        .iter()
        .map(|d| d.wall_ms)
        .filter(|ms| *ms > 0)
        .collect();
    (totals.len() >= 2).then(|| totals.iter().sum::<i64>() / totals.len() as i64)
}
pub fn clock(ms: i64, day_end: i64) -> String {
    if ms == day_end {
        return "24:00".into();
    }
    glib::DateTime::from_unix_local(ms / 1000)
        .expect("session date")
        .format("%H:%M")
        .expect("clock format")
        .to_string()
}
pub fn accessible_duration(ms: i64) -> String {
    let minutes = (ms + 30_000) / 60_000;
    if minutes < 60 {
        format!(
            "{minutes} {}",
            if minutes == 1 { "minute" } else { "minutes" }
        )
    } else {
        let hours = minutes / 60;
        let m = minutes % 60;
        format!(
            "{hours} {} {m} {}",
            if hours == 1 { "hour" } else { "hours" },
            if m == 1 { "minute" } else { "minutes" }
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_calendar_rules() {
        let tz = glib::TimeZone::new(Some("America/New_York"));
        for (month, day, hours) in [(3, 8, 23), (11, 1, 25)] {
            let date = glib::DateTime::new(&tz, 2026, month, day, 0, 0, 0.).unwrap();
            let q = query(&date);
            assert_eq!(q.end_ms - q.start_ms, hours * 3_600_000);
            assert_eq!(clock(q.end_ms, q.end_ms), "24:00");
        }
        let now = glib::DateTime::new(&tz, 2026, 11, 5, 14, 30, 0.).unwrap();
        let queries = usual_queries(&now, &now);
        assert_eq!(queries.len(), 4);
        for q in queries {
            let end = glib::DateTime::from_unix_utc(q.end_ms / 1000)
                .unwrap()
                .to_timezone(&tz)
                .unwrap();
            assert_eq!((end.hour(), end.minute()), (14, 30));
        }
        assert_eq!(accessible_duration(89_999), "1 minute");
        assert_eq!(accessible_duration(90_000), "2 minutes");
        assert_eq!(accessible_duration(3_570_000), "1 hour 0 minutes");
    }
    #[test]
    fn usual_ignores_missing_days() {
        let detail = |wall_ms| DayDetail {
            start_ms: 0,
            end_ms: 1000,
            wall_ms,
            device_ms: wall_ms,
            devices: vec![],
        };
        assert_eq!(
            usual_total(&[detail(100), detail(300), detail(0), detail(0)]),
            Some(200)
        );
        assert_eq!(
            usual_total(&[detail(100), detail(0), detail(0), detail(0)]),
            None
        );
    }
}
