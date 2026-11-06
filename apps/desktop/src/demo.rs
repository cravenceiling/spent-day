//! The `--demo` dataset. Stub until unit 1 moves the Linux dataset here: every day is empty.

use spentday_core::{DayDetail, DayQuery, Summary, SummaryQuery, SyncConfig};
use std::sync::Mutex;

pub struct Demo {
    pub sync: Mutex<Option<SyncConfig>>,
}

impl Demo {
    pub fn new() -> Self {
        Demo {
            sync: Mutex::new(None),
        }
    }

    pub fn day(&self, q: DayQuery) -> DayDetail {
        DayDetail {
            start_ms: q.start_ms,
            end_ms: q.end_ms,
            wall_ms: 0,
            device_ms: 0,
            devices: Vec::new(),
        }
    }

    pub fn summary(&self, q: SummaryQuery) -> Summary {
        Summary {
            start_ms: q.start_ms,
            end_ms: q.end_ms,
            total_ms: 0,
            by_device: Vec::new(),
            apps: Vec::new(),
            sites: Vec::new(),
            buckets: Vec::new(),
        }
    }
}
