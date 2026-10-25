//! What the window reads. The core implements it, and `--demo` swaps in sample data.

use crate::demo::Demo;
use spentday_core::{DeviceInfo, SpentDay, SpentDayError, Summary, SummaryQuery, SyncConfig, SyncReport};

pub trait Store: Send + Sync {
    fn summary(&self, query: SummaryQuery) -> Result<Summary, SpentDayError>;
    fn devices(&self) -> Result<Vec<DeviceInfo>, SpentDayError>;
    fn new_sync_code(&self) -> String;
    fn sync_config(&self) -> Result<Option<SyncConfig>, SpentDayError>;
    fn set_sync_config(&self, config: Option<SyncConfig>) -> Result<(), SpentDayError>;
    fn sync_now(&self) -> Result<SyncReport, SpentDayError>;
}

impl Store for SpentDay {
    fn summary(&self, query: SummaryQuery) -> Result<Summary, SpentDayError> {
        SpentDay::summary(self, query)
    }
    fn devices(&self) -> Result<Vec<DeviceInfo>, SpentDayError> {
        SpentDay::devices(self)
    }
    fn new_sync_code(&self) -> String {
        SpentDay::new_sync_code(self)
    }
    fn sync_config(&self) -> Result<Option<SyncConfig>, SpentDayError> {
        SpentDay::sync_config(self)
    }
    fn set_sync_config(&self, config: Option<SyncConfig>) -> Result<(), SpentDayError> {
        SpentDay::set_sync_config(self, config)
    }
    fn sync_now(&self) -> Result<SyncReport, SpentDayError> {
        SpentDay::sync_now(self)
    }
}

impl Store for Demo {
    fn summary(&self, query: SummaryQuery) -> Result<Summary, SpentDayError> {
        Ok(Demo::summary(self, query))
    }
    fn devices(&self) -> Result<Vec<DeviceInfo>, SpentDayError> {
        Ok(self.devices.clone())
    }
    fn new_sync_code(&self) -> String {
        "7KQ2-M3XP-R2D9-WF4H-B8TN-C6JV-Q1ZE-HY5G-4MRA-T0VD-9XKP-E3NW-J7FB-2QHS".into()
    }
    fn sync_config(&self) -> Result<Option<SyncConfig>, SpentDayError> {
        Ok(self.sync.lock().unwrap().clone())
    }
    fn set_sync_config(&self, config: Option<SyncConfig>) -> Result<(), SpentDayError> {
        *self.sync.lock().unwrap() = config;
        Ok(())
    }
    fn sync_now(&self) -> Result<SyncReport, SpentDayError> {
        std::thread::sleep(std::time::Duration::from_millis(700));
        if self.sync.lock().unwrap().is_none() {
            return Err(SpentDayError::SyncNotConfigured);
        }
        Ok(SyncReport { pushed_spans: 18, pulled_spans: 342, skipped_batches: 0, finished_at_ms: crate::tracker::now_ms() })
    }
}
