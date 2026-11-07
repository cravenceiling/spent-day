use super::browser::{BrowserPages, BrowserTab};
use spentday_core::{Platform, SpentDay, SummaryQuery};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "spentday-macos-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    pub fn core(&self) -> Arc<SpentDay> {
        SpentDay::open(
            self.0.join("spentday.db").to_str().unwrap().into(),
            "Test Mac".into(),
            Platform::Macos,
        )
        .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
pub fn t0() -> i64 {
    (super::now_ms() / 3_600_000 - 1) * 3_600_000
}
pub fn summary(core: &SpentDay) -> spentday_core::Summary {
    core.summary(SummaryQuery {
        start_ms: t0() - 3_600_000,
        end_ms: t0() + 3_600_000,
        bucket_ms: 3_600_000,
        device_id: None,
    })
    .unwrap()
}
pub fn sites(core: &SpentDay, app: &str) -> BTreeMap<String, i64> {
    let summary = summary(core);
    let Some(row) = summary
        .apps
        .iter()
        .find(|r| r.app_ids.iter().any(|id| id == app))
    else {
        return BTreeMap::new();
    };
    if row.domains.is_empty() {
        return BTreeMap::from([("".into(), row.total_ms)]);
    }
    row.domains
        .iter()
        .map(|d| (d.domain.clone(), d.total_ms))
        .collect()
}
pub fn expected(entries: &[(&str, i64)]) -> BTreeMap<String, i64> {
    entries.iter().map(|(s, n)| (s.to_string(), *n)).collect()
}
pub fn page(url: &str) -> BrowserTab {
    BrowserTab::Page(url.into())
}
struct State {
    answer: Result<BrowserTab, i32>,
    held: bool,
    window: Option<i64>,
    asked: usize,
}
pub struct Fake {
    state: Arc<(Mutex<State>, Condvar)>,
    clock: Arc<Mutex<Instant>>,
    answers: mpsc::Receiver<()>,
    started: mpsc::Receiver<()>,
}
impl Fake {
    pub fn new() -> (Self, BrowserPages) {
        let state = Arc::new((
            Mutex::new(State {
                answer: Ok(BrowserTab::NoPage),
                held: false,
                window: Some(1),
                asked: 0,
            }),
            Condvar::new(),
        ));
        let clock = Arc::new(Mutex::new(Instant::now()));
        let (done, answers) = mpsc::channel();
        let (start, started) = mpsc::channel();
        let (query, window, time) = (state.clone(), state.clone(), clock.clone());
        let pages = BrowserPages::spawn(
            Arc::new(move |_| window.0.lock().unwrap().window),
            Arc::new(move || *time.lock().unwrap()),
            move || {
                move |_: &str| {
                    let mut state = query.0.lock().unwrap();
                    state.asked += 1;
                    let _ = start.send(());
                    while state.held {
                        state = query.1.wait(state).unwrap();
                    }
                    state.answer.clone()
                }
            },
            move || {
                let _ = done.send(());
            },
        );
        (
            Self {
                state,
                clock,
                answers,
                started,
            },
            pages,
        )
    }
    pub fn show(&self, answer: BrowserTab) {
        self.state.0.lock().unwrap().answer = Ok(answer);
    }
    pub fn error(&self, code: i32) {
        self.state.0.lock().unwrap().answer = Err(code);
    }
    pub fn window(&self, window: Option<i64>) {
        self.state.0.lock().unwrap().window = window;
    }
    pub fn hold(&self) {
        self.state.0.lock().unwrap().held = true;
    }
    pub fn release(&self) {
        self.state.0.lock().unwrap().held = false;
        self.state.1.notify_all();
    }
    pub fn wait(&self) {
        self.answers.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    pub fn started(&self) {
        self.started.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    pub fn asked(&self) -> usize {
        self.state.0.lock().unwrap().asked
    }
    pub fn advance(&self, millis: u64) {
        *self.clock.lock().unwrap() += Duration::from_millis(millis);
    }
}
