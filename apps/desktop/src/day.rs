//! Calendar rules in the user's time zone. Stubs until unit 1 ports `apps/linux/src/day.rs` to
//! jiff; the signatures are final. Never add 86,400,000 ms for a day: DST days are 23 or 25 h.

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use spentday_core::{DayDetail, DayQuery};

pub fn today(tz: &TimeZone) -> Date {
    Timestamp::now().to_zoned(tz.clone()).date()
}

/// Local midnight to the next local midnight.
pub fn query(date: Date, tz: &TimeZone) -> DayQuery {
    let _ = (date, tz);
    DayQuery {
        start_ms: 0,
        end_ms: 0,
    }
}

/// `date` moved by whole calendar days.
pub fn step(date: Date, days: i32) -> Date {
    let _ = days;
    date
}

/// The same weekday in the 4 weeks before `date`; on today each ends at the current local clock.
pub fn usual_queries(date: Date, now: Timestamp, tz: &TimeZone) -> Vec<DayQuery> {
    let _ = (date, now, tz);
    Vec::new()
}

pub fn usual_total(days: &[DayDetail]) -> Option<i64> {
    let totals: Vec<_> = days
        .iter()
        .map(|d| d.wall_ms)
        .filter(|ms| *ms > 0)
        .collect();
    (totals.len() >= 2).then(|| totals.iter().sum::<i64>() / totals.len() as i64)
}

/// `HH:MM`, or `24:00` when `ms` is the day's end.
pub fn clock(ms: i64, day_end: i64, tz: &TimeZone) -> String {
    let _ = (ms, day_end, tz);
    String::new()
}

/// `6 hours 5 minutes`, for accessibility labels.
pub fn accessible_duration(ms: i64) -> String {
    let _ = ms;
    String::new()
}

/// `Wednesday, 28 October`.
pub fn header_date(date: Date) -> String {
    let _ = date;
    String::new()
}

/// `Wednesday`.
pub fn weekday_name(date: Date) -> &'static str {
    let _ = date;
    ""
}
