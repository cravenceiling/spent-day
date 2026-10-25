use gtk::glib;
use spentday_core::SummaryQuery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Today,
    Yesterday,
    Week,
}

pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 24 * HOUR_MS;

impl Range {
    pub const ALL: [Range; 3] = [Range::Today, Range::Yesterday, Range::Week];

    pub fn id(self) -> &'static str {
        match self {
            Range::Today => "today",
            Range::Yesterday => "yesterday",
            Range::Week => "week",
        }
    }

    pub fn from_id(id: &str) -> Range {
        Range::ALL.into_iter().find(|r| r.id() == id).unwrap_or(Range::Today)
    }

    pub fn label(self) -> &'static str {
        match self {
            Range::Today => "Today",
            Range::Yesterday => "Yesterday",
            Range::Week => "Last 7 Days",
        }
    }

    /// Local midnight to local midnight. The core only sees milliseconds, so time zones stay here.
    pub fn bounds(self, now: &glib::DateTime) -> (glib::DateTime, glib::DateTime) {
        let midnight = glib::DateTime::from_local(now.year(), now.month(), now.day_of_month(), 0, 0, 0.0)
            .expect("local midnight");
        let days = |n| midnight.add_days(n).expect("date in range");
        match self {
            Range::Today => (midnight.clone(), days(1)),
            Range::Yesterday => (days(-1), midnight.clone()),
            Range::Week => (days(-6), days(1)),
        }
    }

    pub fn bucket_ms(self) -> i64 {
        if self == Range::Week { DAY_MS } else { HOUR_MS }
    }

    pub fn query(self, now: &glib::DateTime, device_id: Option<String>) -> SummaryQuery {
        let (start, end) = self.bounds(now);
        SummaryQuery {
            start_ms: start.to_unix() * 1000,
            end_ms: end.to_unix() * 1000,
            bucket_ms: self.bucket_ms(),
            device_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_start_at_local_midnight() {
        let now = glib::DateTime::from_local(2026, 10, 19, 15, 42, 7.0).unwrap();
        let today = Range::Today.query(&now, None);
        let midnight = glib::DateTime::from_local(2026, 10, 19, 0, 0, 0.0).unwrap();
        assert_eq!(today.start_ms, midnight.to_unix() * 1000);
        assert_eq!(today.end_ms - today.start_ms, DAY_MS);
        assert_eq!(today.bucket_ms, HOUR_MS);
        let yesterday = Range::Yesterday.query(&now, None);
        assert_eq!(yesterday.end_ms, today.start_ms);
        let week = Range::Week.query(&now, Some("d".into()));
        assert_eq!((week.end_ms, week.end_ms - week.start_ms, week.bucket_ms), (today.end_ms, 7 * DAY_MS, DAY_MS));
        assert_eq!(week.device_id.as_deref(), Some("d"));
    }
}
