//! Calendar rules in an explicit time zone, including short and long DST days.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use spentday_core::{DayDetail, DayQuery};

pub fn today(tz: &TimeZone) -> Date {
    Timestamp::now().to_zoned(tz.clone()).date()
}

/// Local midnight to the next local midnight.
pub fn query(date: Date, tz: &TimeZone) -> DayQuery {
    DayQuery {
        start_ms: date
            .to_zoned(tz.clone())
            .expect("local midnight")
            .timestamp()
            .as_millisecond(),
        end_ms: step(date, 1)
            .to_zoned(tz.clone())
            .expect("next midnight")
            .timestamp()
            .as_millisecond(),
    }
}

pub fn step(date: Date, days: i32) -> Date {
    date.checked_add(i64::from(days).days())
        .expect("calendar day")
}

/// The prior four matching weekdays, clipped to the current local clock only on today.
pub fn usual_queries(date: Date, now: Timestamp, tz: &TimeZone) -> Vec<DayQuery> {
    let now = now.to_zoned(tz.clone());
    (1..=4)
        .map(|week| {
            let previous = step(date, -7 * week);
            let mut q = query(previous, tz);
            if date == now.date() {
                q.end_ms = previous
                    .to_datetime(now.time())
                    .to_zoned(tz.clone())
                    .expect("historic local clock")
                    .timestamp()
                    .as_millisecond();
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

/// `HH:MM`, with `24:00` at the query's end.
pub fn clock(ms: i64, day_end: i64, tz: &TimeZone) -> String {
    if ms == day_end {
        return "24:00".into();
    }
    let local = Timestamp::from_millisecond(ms)
        .expect("session timestamp")
        .to_zoned(tz.clone());
    format!("{:02}:{:02}", local.hour(), local.minute())
}

pub fn accessible_duration(ms: i64) -> String {
    let minutes = (ms.max(0) + 30_000) / 60_000;
    let unit = |n, singular, plural| if n == 1 { singular } else { plural };
    if minutes < 60 {
        format!("{minutes} {}", unit(minutes, "minute", "minutes"))
    } else {
        let (hours, m) = (minutes / 60, minutes % 60);
        format!(
            "{hours} {} {m} {}",
            unit(hours, "hour", "hours"),
            unit(m, "minute", "minutes")
        )
    }
}

pub fn header_date(date: Date) -> String {
    let month = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ][date.month() as usize - 1];
    format!("{}, {} {month}", weekday_name(date), date.day())
}

pub fn weekday_name(date: Date) -> &'static str {
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ][date.weekday().to_monday_zero_offset() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn literal_calendar_rules() {
        let tz = TimeZone::get("America/New_York").unwrap();
        for (month, day, length) in [(3, 8, 82_800_000), (11, 1, 90_000_000)] {
            let q = query(date(2026, month, day), &tz);
            assert_eq!(q.end_ms - q.start_ms, length);
            assert_eq!(clock(q.start_ms, q.end_ms, &tz), "00:00");
            assert_eq!(clock(q.end_ms, q.end_ms, &tz), "24:00");
        }
        let selected = date(2026, 11, 5);
        let now = selected
            .at(14, 30, 0, 0)
            .to_zoned(tz.clone())
            .unwrap()
            .timestamp();
        let queries = usual_queries(selected, now, &tz);
        assert_eq!(queries.len(), 4);
        for (week, q) in queries.iter().enumerate() {
            let end = Timestamp::from_millisecond(q.end_ms)
                .unwrap()
                .to_zoned(tz.clone());
            assert_eq!((end.hour(), end.minute()), (14, 30));
            assert_eq!(end.date(), step(selected, -7 * (week as i32 + 1)));
        }
        let late = date(2026, 10, 28)
            .at(23, 38, 0, 0)
            .to_zoned(tz.clone())
            .unwrap()
            .timestamp()
            .as_millisecond();
        assert_eq!(
            clock(late, query(date(2026, 10, 28), &tz).end_ms, &tz),
            "23:38"
        );
        assert_eq!(accessible_duration(89_999), "1 minute");
        assert_eq!(accessible_duration(90_000), "2 minutes");
        assert_eq!(accessible_duration(3_570_000), "1 hour 0 minutes");
        assert_eq!(accessible_duration(-60_000), "0 minutes");
        assert_eq!(header_date(date(2026, 10, 28)), "Wednesday, 28 October");
    }

    #[test]
    fn history_uses_local_clock_across_dst_and_full_past_days() {
        let tz = TimeZone::get("America/New_York").unwrap();
        for selected in [date(2026, 3, 15), date(2026, 11, 8)] {
            let now = selected
                .at(14, 30, 12, 123_000_000)
                .to_zoned(tz.clone())
                .unwrap()
                .timestamp();
            for q in usual_queries(selected, now, &tz) {
                let end = Timestamp::from_millisecond(q.end_ms)
                    .unwrap()
                    .to_zoned(tz.clone());
                assert_eq!(
                    (end.hour(), end.minute(), end.second(), end.millisecond()),
                    (14, 30, 12, 123)
                );
            }
            let past = step(selected, -7);
            let queries = usual_queries(past, now, &tz);
            assert_eq!(queries[0], query(step(past, -7), &tz));
        }
        assert_eq!(step(date(2026, 3, 8), 1), date(2026, 3, 9));
        assert_eq!(step(date(2026, 11, 1), -1), date(2026, 10, 31));
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
