//! The sentences under the headline and the details lines. Stubs until unit 1.

use jiff::tz::TimeZone;
use spentday_core::{DayDetail, DayItem, DeviceDay};

/// `47m more than a usual Wednesday. First screen 07:12, last 23:38.`
pub fn day_sentence(
    detail: &DayDetail,
    usual_ms: Option<i64>,
    weekday: &str,
    tz: &TimeZone,
) -> String {
    let _ = (detail, usual_ms, weekday, tz);
    String::new()
}

/// `MacBook Pro, 6 apps and sites, 08:49 to 21:10.`
pub fn device_sentence(device: &DeviceDay, day_end: i64, tz: &TimeZone) -> String {
    let _ = (device, day_end, tz);
    String::new()
}

pub fn item_sentence(item: &DayItem, day_end: i64, tz: &TimeZone) -> String {
    let _ = (item, day_end, tz);
    String::new()
}

/// `47m more than usual`-style comparison, or `None` without a usual day.
pub fn usual_text(total_ms: i64, usual_ms: Option<i64>, weekday: &str) -> Option<String> {
    let _ = (total_ms, usual_ms, weekday);
    None
}

/// Overlap, longest stretch per device, time after 23:00.
pub fn details_lines(detail: &DayDetail, tz: &TimeZone) -> Vec<String> {
    let _ = (detail, tz);
    Vec::new()
}
