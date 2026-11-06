//! One sync worker at a time; configuration generations keep old results out of the UI.

use crate::app::Shared;
use jiff::Timestamp;
use spentday_core::{SyncConfig, SyncReport};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Manual,
    Auto,
}

#[derive(Default)]
pub struct SyncState {
    pub running: bool,
    pub resync_when_done: bool,
    pub generation: u64,
    pub last: Option<LastSync>,
    pub configured: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LastSync {
    Ok { at: Timestamp, report: SyncReport },
    Failed { at: Timestamp, message: String },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncView {
    pub running: bool,
    pub configured: bool,
    pub last: Option<LastSync>,
}

impl SyncState {
    pub fn view(&self) -> SyncView {
        SyncView {
            running: self.running,
            configured: self.configured,
            last: self.last.clone(),
        }
    }
}

pub fn run(shared: &Arc<Shared>, trigger: Trigger) {
    if trigger == Trigger::Auto && shared.demo {
        return;
    }
    let generation = {
        let mut s = shared.sync.lock().unwrap();
        if s.running {
            return;
        }
        s.running = true;
        s.generation
    };
    broadcast(shared);
    let shared = shared.clone();
    std::thread::spawn(move || {
        let result = match shared.store.sync_config() {
            Ok(Some(_)) => Some(
                shared
                    .store
                    .sync_now()
                    .map_err(|e| crate::format::error("sync", &e)),
            ),
            Ok(None) => None,
            Err(e) => Some(Err(crate::format::error("load sync settings", &e))),
        };
        let again = {
            let mut s = shared.sync.lock().unwrap();
            s.running = false;
            if generation == s.generation {
                s.configured = result.is_some();
                s.last = result.map(|r| match r {
                    Ok(report) => LastSync::Ok {
                        at: Timestamp::now(),
                        report,
                    },
                    Err(message) => LastSync::Failed {
                        at: Timestamp::now(),
                        message,
                    },
                });
            }
            let again = s.resync_when_done && s.configured;
            s.resync_when_done = false;
            again
        };
        broadcast(&shared);
        // A failed pull can still have changed local data before it failed.
        if let Some(ui) = shared.ui.get() {
            ui.dash.update_if(|d, tx| d.refresh(tx));
            ui.panel.update_if(|p, tx| p.refresh(tx));
        }
        if again {
            run(&shared, Trigger::Manual);
        }
    });
}

pub fn configure(
    shared: &Arc<Shared>,
    server: String,
    code: String,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    let shared = shared.clone();
    std::thread::spawn(move || {
        let result = shared
            .store
            .set_sync_config(Some(SyncConfig {
                server_url: server.trim().trim_end_matches('/').to_owned(),
                sync_code: code.trim().to_owned(),
            }))
            .map_err(|e| crate::format::error("save sync settings", &e));
        if result.is_ok() {
            let config = shared
                .store
                .sync_config()
                .map_err(|e| crate::format::error("load sync settings", &e));
            {
                let mut s = shared.sync.lock().unwrap();
                s.generation += 1;
                s.configured = true;
                s.last = None;
                s.resync_when_done = s.running;
            }
            if let Ok(config) = config {
                publish_config(&shared, config);
            }
            broadcast(&shared);
            run(&shared, Trigger::Manual);
        }
        done(result);
    });
}

pub fn create(
    shared: &Arc<Shared>,
    server: String,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    let shared = shared.clone();
    std::thread::spawn(move || {
        let code = shared.store.new_sync_code();
        configure(&shared, server, code, done);
    });
}

pub fn turn_off(shared: &Arc<Shared>, done: impl FnOnce(Result<(), String>) + Send + 'static) {
    let shared = shared.clone();
    std::thread::spawn(move || {
        let result = shared
            .store
            .set_sync_config(None)
            .map_err(|e| crate::format::error("turn off sync", &e));
        if result.is_ok() {
            {
                let mut s = shared.sync.lock().unwrap();
                s.generation += 1;
                s.configured = false;
                s.last = None;
                s.resync_when_done = false;
            }
            publish_config(&shared, None);
            broadcast(&shared);
        }
        done(result);
    });
}

fn publish_config(shared: &Arc<Shared>, config: Option<SyncConfig>) {
    if let Some(ui) = shared.ui.get() {
        ui.dash.update(move |d, _| d.settings.apply_config(config));
    }
}

pub fn broadcast(shared: &Arc<Shared>) {
    if let Some(ui) = shared.ui.get() {
        // Read when the update runs so racing worker notifications cannot regress a view.
        ui.dash
            .update(|d, _| d.settings.sync = d.shared.sync.lock().unwrap().view());
        ui.panel
            .update(|p, _| p.sync = p.shared.sync.lock().unwrap().view());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo::Demo, icons, store::Store};
    use spentday_core::{DayDetail, DayQuery, SpentDayError, Summary, SummaryQuery};
    use std::sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;

    struct Fake {
        config: Mutex<Option<SyncConfig>>,
        calls: AtomicUsize,
        started: mpsc::Sender<usize>,
        release: Mutex<mpsc::Receiver<()>>,
        fail_off: AtomicBool,
        fail_run: AtomicBool,
    }
    impl Store for Fake {
        fn day(&self, q: DayQuery) -> Result<DayDetail, SpentDayError> {
            Ok(Demo::new().day(q))
        }
        fn summary(&self, q: SummaryQuery) -> Result<Summary, SpentDayError> {
            Ok(Demo::new().summary(q))
        }
        fn new_sync_code(&self) -> String {
            "new-code".into()
        }
        fn sync_config(&self) -> Result<Option<SyncConfig>, SpentDayError> {
            Ok(self.config.lock().unwrap().clone())
        }
        fn set_sync_config(&self, config: Option<SyncConfig>) -> Result<(), SpentDayError> {
            if config.is_none() && self.fail_off.load(Ordering::SeqCst) {
                return Err(SpentDayError::Storage("read only".into()));
            }
            *self.config.lock().unwrap() = config;
            Ok(())
        }
        fn sync_now(&self) -> Result<SyncReport, SpentDayError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            self.started.send(n).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            if self.fail_run.load(Ordering::SeqCst) {
                return Err(SpentDayError::Network("hostile body".into()));
            }
            Ok(SyncReport {
                pushed_spans: n as u64,
                pulled_spans: 342,
                skipped_batches: 0,
                finished_at_ms: 0,
            })
        }
    }
    fn shared(store: Arc<dyn Store>, demo: bool) -> Arc<Shared> {
        Arc::new(Shared {
            store,
            demo,
            tz: jiff::tz::TimeZone::UTC,
            icons: icons::Seeds::default(),
            sync: Mutex::default(),
            ui: OnceLock::new(),
        })
    }
    fn fake() -> (
        Arc<Shared>,
        Arc<Fake>,
        mpsc::Receiver<usize>,
        mpsc::Sender<()>,
    ) {
        let (started, receive) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let fake = Arc::new(Fake {
            config: Mutex::new(Some(SyncConfig {
                server_url: "https://sync.example.com".into(),
                sync_code: "old-code".into(),
            })),
            calls: AtomicUsize::new(0),
            started,
            release: Mutex::new(wait),
            fail_off: AtomicBool::new(false),
            fail_run: AtomicBool::new(false),
        });
        let shared = shared(fake.clone(), false);
        shared.sync.lock().unwrap().configured = true;
        (shared, fake, receive, release)
    }
    fn wait_idle(shared: &Shared) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while shared.sync.lock().unwrap().running {
            assert!(std::time::Instant::now() < deadline, "sync did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn duplicate_run_is_dropped() {
        let (s, f, started, release) = fake();
        run(&s, Trigger::Manual);
        assert_eq!(started.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        run(&s, Trigger::Manual);
        run(&s, Trigger::Auto);
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        assert!(!s.sync.lock().unwrap().resync_when_done);
        release.send(()).unwrap();
        wait_idle(&s);
        assert!(matches!(
            s.sync.lock().unwrap().last,
            Some(LastSync::Ok { .. })
        ));
    }
    #[test]
    fn configure_during_run_discards_old_result_and_resyncs_once() {
        let (s, f, started, release) = fake();
        run(&s, Trigger::Manual);
        assert_eq!(started.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        let (done, result) = mpsc::channel();
        configure(
            &s,
            " https://other.example.com/// ".into(),
            " new-code ".into(),
            move |r| done.send(r).unwrap(),
        );
        result
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(s.sync.lock().unwrap().generation, 1);
        assert!(s.sync.lock().unwrap().resync_when_done);
        assert_eq!(
            f.sync_config().unwrap().unwrap().server_url,
            "https://other.example.com"
        );
        release.send(()).unwrap();
        assert_eq!(started.recv_timeout(Duration::from_secs(2)).unwrap(), 2);
        assert!(
            s.sync.lock().unwrap().last.is_none(),
            "old account's result was published"
        );
        release.send(()).unwrap();
        wait_idle(&s);
        assert_eq!(f.calls.load(Ordering::SeqCst), 2);
        assert!(
            matches!(&s.sync.lock().unwrap().last, Some(LastSync::Ok { report, .. }) if report.pushed_spans == 2)
        );
    }
    #[test]
    fn failed_turn_off_keeps_configuration_and_generation() {
        let (s, f, _, _) = fake();
        f.fail_off.store(true, Ordering::SeqCst);
        let (done, result) = mpsc::channel();
        turn_off(&s, move |r| done.send(r).unwrap());
        assert_eq!(
            result.recv_timeout(Duration::from_secs(2)).unwrap(),
            Err("Spent Day couldn't read its database. read only".into())
        );
        assert!(f.sync_config().unwrap().is_some());
        assert!(s.sync.lock().unwrap().configured);
        assert_eq!(s.sync.lock().unwrap().generation, 0);
    }
    #[test]
    fn turn_off_invalidates_running_result_without_resync() {
        let (s, f, started, release) = fake();
        run(&s, Trigger::Manual);
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let (done, result) = mpsc::channel();
        turn_off(&s, move |r| done.send(r).unwrap());
        result
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        release.send(()).unwrap();
        wait_idle(&s);
        assert!(s.sync.lock().unwrap().last.is_none());
        assert!(!s.sync.lock().unwrap().configured);
        assert!(f.sync_config().unwrap().is_none());
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn failure_uses_fixed_message_and_clears_running() {
        let (s, f, started, release) = fake();
        f.fail_run.store(true, Ordering::SeqCst);
        run(&s, Trigger::Manual);
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        release.send(()).unwrap();
        wait_idle(&s);
        assert!(
            matches!(&s.sync.lock().unwrap().last, Some(LastSync::Failed { message, .. })
            if message == "Couldn't reach the sync server or read its answer. Check the address and your connection.")
        );
    }
    #[test]
    fn demo_manual_sync_reports_18_sent_and_342_received() {
        let store = Arc::new(Demo::new());
        store
            .set_sync_config(Some(SyncConfig {
                server_url: "https://sync.example.com".into(),
                sync_code: store.new_sync_code(),
            }))
            .unwrap();
        let s = shared(store, true);
        run(&s, Trigger::Auto);
        assert!(!s.sync.lock().unwrap().running);
        run(&s, Trigger::Manual);
        wait_idle(&s);
        assert!(
            matches!(&s.sync.lock().unwrap().last, Some(LastSync::Ok { report, .. })
            if report.pushed_spans == 18 && report.pulled_spans == 342)
        );
    }
}
