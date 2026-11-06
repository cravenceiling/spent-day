use super::{
    browser::BrowserPages,
    front,
    presence::{self, LastSeen},
    recorder::Recorder,
};
use block2::RcBlock;
use objc2::Message;
use objc2::{
    rc::{Retained, autoreleasepool},
    runtime::ProtocolObject,
};
use objc2_app_kit::*;
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSObjectProtocol,
    NSString,
};
use spentday_core::{SpentDay, SpentDayError};
use std::{
    collections::HashSet,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Hidden {
    Asleep,
    DisplayAsleep,
    Locked,
    Screensaver,
    SwitchedOut,
}
#[derive(Clone, Copy)]
enum Event {
    Activate,
    Hide(Hidden),
    Show(Hidden),
    Tick,
    Stop,
}

pub struct Tracking {
    events: mpsc::Sender<Event>,
    done: mpsc::Receiver<()>,
    worker: thread::JoinHandle<()>,
}
impl Tracking {
    pub fn stop(self) {
        let _ = self.events.send(Event::Stop);
        if self.done.recv_timeout(Duration::from_secs(3)).is_ok() {
            let _ = self.worker.join();
        } else {
            eprintln!("Spent Day collector did not stop within 3 seconds");
        }
    }
}
pub fn start(core: Arc<SpentDay>, on_fatal: Box<dyn FnOnce(String) + Send>) -> Tracking {
    let (events, receive) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let answers = events.clone();
    let observers = events.clone();
    let worker = thread::Builder::new()
        .name("spentday-collector".into())
        .spawn(move || {
            let mut collector = autoreleasepool(|_| {
                let subscriptions = Observers::new(observers);
                let recorder = Recorder::new(
                    core.clone(),
                    BrowserPages::new(move || {
                        let _ = answers.send(Event::Tick);
                    }),
                );
                (
                    Collector {
                        core,
                        recorder,
                        hidden: HashSet::new(),
                        last_seen: LastSeen::default(),
                    },
                    subscriptions,
                )
            });
            drop(on_fatal);
            let mut due = Instant::now();
            loop {
                let event = receive
                    .recv_timeout(due.saturating_duration_since(Instant::now()))
                    .unwrap_or(Event::Tick);
                let stop = matches!(event, Event::Stop);
                autoreleasepool(|_| match event {
                    Event::Activate => {
                        collector.0.recorder.activated();
                        collector.0.tick();
                    }
                    Event::Hide(reason) => {
                        collector.0.hidden.insert(reason);
                        collector.0.flush();
                    }
                    Event::Show(reason) => {
                        collector.0.hidden.remove(&reason);
                        collector.0.tick();
                    }
                    Event::Tick => collector.0.tick(),
                    Event::Stop => collector.0.flush(),
                });
                if stop {
                    break;
                }
                if Instant::now() >= due {
                    due = Instant::now() + Duration::from_secs(5);
                }
            }
            autoreleasepool(|_| drop(collector));
            let _ = finished.send(());
        })
        .expect("start collector worker");
    Tracking {
        events,
        done,
        worker,
    }
}
struct Collector {
    core: Arc<SpentDay>,
    recorder: Recorder,
    hidden: HashSet<Hidden>,
    last_seen: LastSeen,
}
impl Collector {
    fn flush(&self) {
        log("flush", self.core.flush(super::now_ms()));
    }
    fn tick(&mut self) {
        if let Some((locked, switched)) = front::session_state() {
            for (reason, set) in [(Hidden::Locked, locked), (Hidden::SwitchedOut, switched)] {
                if !set {
                    self.hidden.remove(&reason);
                } else if self.hidden.insert(reason) {
                    self.flush();
                }
            }
        }
        if !self.hidden.is_empty() {
            return;
        }
        let now = super::now_ms();
        let away = self
            .last_seen
            .away_since(now, presence::last_input_ms(now), || {
                front::current().is_some_and(|front| {
                    presence::holds_display_awake(front.pid, &presence::current_assertions())
                })
            });
        log(
            "heartbeat",
            if let Some(away) = away {
                self.core.away_since(away, now)
            } else {
                self.recorder.record(
                    front::current(),
                    now,
                    super::read_pref(super::Pref::RecordSafariSites).unwrap_or(false),
                )
            },
        );
    }
}
fn log(what: &str, result: Result<(), SpentDayError>) {
    if let Err(error) = result {
        let message = match error {
            SpentDayError::Network(_) => "couldn't reach the sync server or read its answer".into(),
            SpentDayError::Server { status, .. } => format!("the sync server answered {status}"),
            error => error.to_string(),
        };
        eprintln!("Spent Day {what} failed: {message}");
    }
}
struct Observer {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}
struct Observers(Vec<Observer>);
impl Observers {
    fn new(events: mpsc::Sender<Event>) -> Self {
        let mut observers = Self(Vec::new());
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let distributed = NSDistributedNotificationCenter::defaultCenter();
        unsafe {
            for (name, event) in [
                (
                    NSWorkspaceDidActivateApplicationNotification,
                    Event::Activate,
                ),
                (
                    NSWorkspaceWillSleepNotification,
                    Event::Hide(Hidden::Asleep),
                ),
                (NSWorkspaceDidWakeNotification, Event::Show(Hidden::Asleep)),
                (
                    NSWorkspaceScreensDidSleepNotification,
                    Event::Hide(Hidden::DisplayAsleep),
                ),
                (
                    NSWorkspaceScreensDidWakeNotification,
                    Event::Show(Hidden::DisplayAsleep),
                ),
                (
                    NSWorkspaceSessionDidResignActiveNotification,
                    Event::Hide(Hidden::SwitchedOut),
                ),
                (
                    NSWorkspaceSessionDidBecomeActiveNotification,
                    Event::Show(Hidden::SwitchedOut),
                ),
            ] {
                observers.add(&workspace, name, event, events.clone());
            }
        }
        for (name, event) in [
            ("com.apple.screenIsLocked", Event::Hide(Hidden::Locked)),
            ("com.apple.screenIsUnlocked", Event::Show(Hidden::Locked)),
            (
                "com.apple.screensaver.didstart",
                Event::Hide(Hidden::Screensaver),
            ),
            (
                "com.apple.screensaver.didstop",
                Event::Show(Hidden::Screensaver),
            ),
        ] {
            observers.add(
                &distributed,
                &NSString::from_str(name),
                event,
                events.clone(),
            );
        }
        observers
    }
    fn add(
        &mut self,
        center: &NSNotificationCenter,
        name: &NSString,
        event: Event,
        events: mpsc::Sender<Event>,
    ) {
        let block = RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
            let _ = events.send(event);
        });
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        };
        self.0.push(Observer {
            center: center.retain(),
            token,
        });
    }
}
impl Drop for Observers {
    fn drop(&mut self) {
        for Observer { center, token } in &self.0 {
            unsafe {
                center.removeObserver(AsRef::<objc2::runtime::AnyObject>::as_ref(&**token));
            }
        }
    }
}
