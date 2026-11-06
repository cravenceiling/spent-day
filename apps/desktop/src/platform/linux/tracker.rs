//! Polls the focused window and feeds heartbeats to the core.

use crate::platform::linux::collector::{Collector, Kind, Window};
use crate::platform::linux::desktop::Entries;
use crate::platform::linux::idle::{self, Idle};
use crate::platform::linux::media::{Presence, Snapshot, State};
use crate::platform::linux::session;
use spentday_core::{Activity, SpentDay};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const POLL: Duration = Duration::from_secs(1);
const HEARTBEAT_MS: i64 = 5_000;
const AWAY_AFTER_MS: u64 = 180_000;
/// How long a snapshot that found the user present keeps them present while idle. Later ones
/// that find nothing or cannot tell only fail to renew it, so a player between tracks loses no
/// time and a hung check cannot keep the user present for good.
const PRESENT_TTL_MS: i64 = 30_000;
const STOP_WAIT: Duration = Duration::from_secs(3);

pub enum Event {
    Sleep(bool),
    Locked(bool),
    Quit,
}

pub struct Tracker {
    events: Sender<Event>,
    thread: JoinHandle<()>,
}

impl Tracker {
    /// The receiver gets one message once the collector has started, or the reason it did not.
    pub fn spawn(spentday: Arc<SpentDay>) -> (Tracker, Receiver<Result<(), String>>) {
        let (events, rx) = mpsc::channel();
        let (started, started_rx) = mpsc::channel();
        session::watch(events.clone());
        let thread = thread::Builder::new()
            .name("tracker".into())
            .spawn(move || {
                let away_after_ms = away_after_ms();
                let collector = open_collector();
                let _ = started.send(collector.as_ref().map(|_| ()).map_err(Clone::clone));
                let collector = collector.ok().flatten();
                let idle = idle::open(
                    collector.as_ref().map(|(kind, _)| *kind),
                    Duration::from_millis(away_after_ms),
                );
                let feed = Feed::new(spentday, away_after_ms);
                track(
                    rx,
                    feed,
                    collector.map(|(_, collector)| collector),
                    idle,
                    Presence::spawn(),
                );
            })
            .expect("spawn tracker thread");
        (Tracker { events, thread }, started_rx)
    }

    /// Closes the open span and waits for the thread to finish, but not for longer than
    /// `STOP_WAIT`, so quitting never hangs on a compositor or idle service that stopped
    /// answering. The thread finishes its current poll first, whose calls time out on their own.
    pub fn stop(self) {
        let _ = self.events.send(Event::Quit);
        let deadline = Instant::now() + STOP_WAIT;
        while !self.thread.is_finished() {
            if Instant::now() >= deadline {
                eprintln!(
                    "spentday: the tracker did not stop in time; the open span ends at its last heartbeat"
                );
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.thread.join();
    }
}

/// Polls the focused window and idle time every `POLL` until `Quit`. Presence checks run on
/// their own thread, so this loop only hands them the focused app and reads what they found.
fn track(
    events: Receiver<Event>,
    mut feed: Feed,
    mut collector: Option<Box<dyn Collector>>,
    mut idle: Box<dyn Idle>,
    presence: Presence,
) {
    let mut paused = Paused::default();
    let mut last_error = None;
    loop {
        match events.recv_timeout(POLL) {
            Ok(Event::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Event::Sleep(asleep)) => paused.asleep = asleep,
            Ok(Event::Locked(locked)) => paused.locked = locked,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if paused.any() {
            feed.pause(now_ms());
            continue;
        }
        let Some(collector) = collector.as_mut() else {
            continue;
        };
        let window = collector.active().unwrap_or_else(|e| {
            if last_error.as_ref() != Some(&e) {
                eprintln!("spentday: {}: {e}", collector.name());
                last_error = Some(e);
            }
            None
        });
        presence.focus(window.as_ref().map(|w| w.app_id.as_str()));
        feed.poll(now_ms(), window, idle.idle_ms(), presence.latest());
    }
    feed.pause(now_ms());
}

/// Turns what the tracker sees on each poll into heartbeats, and into `away_since` once the
/// user has been idle for `away_after_ms` with nothing keeping them present.
struct Feed {
    spentday: Arc<SpentDay>,
    entries: Entries,
    away_after_ms: u64,
    /// The last heartbeat's time and window. `None` after a flush or while away.
    last: Option<(i64, Option<Window>)>,
    away: bool,
    /// When the latest snapshot that found the user present was taken.
    present_at: Option<i64>,
}

impl Feed {
    fn new(spentday: Arc<SpentDay>, away_after_ms: u64) -> Feed {
        Feed {
            spentday,
            entries: Entries::new(),
            away_after_ms,
            last: None,
            away: false,
            present_at: None,
        }
    }

    /// `idle_ms` is `None` when nothing can tell, and the user then counts as present.
    /// `presence` is the latest snapshot, taken idle or not, so a video that ends before the
    /// idle threshold still counts up to its end.
    fn poll(
        &mut self,
        now: i64,
        window: Option<Window>,
        idle_ms: Option<u64>,
        presence: Option<Snapshot>,
    ) {
        if let Some(Snapshot {
            state: State::Present,
            observed_at_ms,
        }) = presence
        {
            self.present_at = Some(observed_at_ms);
        }
        let kept_present = self.present_at.is_some_and(|at| now <= at + PRESENT_TTL_MS);
        if let Some(idle_ms) = idle_ms.filter(|ms| *ms >= self.away_after_ms && !kept_present) {
            if !self.away {
                // The user left at their last input, or when a video they watched ended.
                let left = (now - idle_ms as i64).max(self.present_at.unwrap_or(i64::MIN));
                log(self.spentday.away_since(left, now));
                self.away = true;
                self.last = None;
            }
            return;
        }
        self.away = false;
        let due = match &self.last {
            None => true,
            Some((at, seen)) => *seen != window || now - at >= HEARTBEAT_MS,
        };
        if !due {
            return;
        }
        let activity = window.clone().map(|w| Activity {
            app_name: self.entries.name(&w.app_id),
            app_id: w.app_id,
            web: None,
        });
        log(self.spentday.heartbeat(now, activity));
        self.last = Some((now, window));
    }

    /// Closes the open span, for sleep, lock and quit.
    fn pause(&mut self, now: i64) {
        if self.last.take().is_some() {
            log(self.spentday.flush(now));
        }
    }
}

#[derive(Default)]
struct Paused {
    asleep: bool,
    locked: bool,
}

impl Paused {
    fn any(&self) -> bool {
        self.asleep || self.locked
    }
}

/// `SPENTDAY_AWAY_AFTER_SECS` shortens the idle threshold, so checks need not wait 3 minutes.
fn away_after_ms() -> u64 {
    std::env::var("SPENTDAY_AWAY_AFTER_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map_or(AWAY_AFTER_MS, |s| s * 1000)
}

type Opened = (Kind, Box<dyn Collector>);

/// `Ok(None)` without a supported display server. `Err` when the collector for this session
/// failed to start.
fn open_collector() -> Result<Option<Opened>, String> {
    let Some(kind) = Kind::detect() else {
        // Sites from the extension only count while a browser window is known to be focused.
        eprintln!(
            "spentday: no supported display server, so nothing is recorded, sites included. Set SPENTDAY_COLLECTOR to x11, sway, hyprland, gnome or kde if detection is wrong."
        );
        return Ok(None);
    };
    match kind.open() {
        Ok(collector) => {
            eprintln!("spentday: tracking with the {} collector", collector.name());
            Ok(Some((kind, collector)))
        }
        Err(e) => {
            eprintln!(
                "spentday: the {kind:?} collector failed to start, so nothing is recorded: {e}"
            );
            Err(e)
        }
    }
}

fn log(result: Result<(), spentday_core::SpentDayError>) {
    if let Err(e) = result {
        eprintln!("spentday: {e}");
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::media::{self, Media};
    use spentday_core::Platform;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    const T0: i64 = 1_700_000_000_000;

    fn feed() -> (tempfile::TempDir, Arc<SpentDay>, Feed) {
        let dir = tempfile::tempdir().unwrap();
        let spentday = SpentDay::open(
            dir.path().join("m.db").display().to_string(),
            "box".into(),
            Platform::Linux,
        )
        .unwrap();
        (dir, spentday.clone(), Feed::new(spentday, AWAY_AFTER_MS))
    }

    /// The snapshot the presence thread would have published by second `s`, checking every 5 s
    /// and finding `state_at` the second it checked.
    fn sampled(s: i64, state_at: impl Fn(i64) -> State) -> Option<Snapshot> {
        let at = s - s % 5;
        Some(Snapshot {
            state: state_at(at),
            observed_at_ms: T0 + at * 1000,
        })
    }

    fn window(app_id: &str) -> Option<Window> {
        Some(Window {
            app_id: app_id.into(),
        })
    }

    /// `(start s, end s, app id)` relative to `T0`, oldest first.
    fn spans(spentday: &SpentDay) -> Vec<(i64, i64, String)> {
        let mut spans: Vec<_> = spentday
            .recent_spans(100)
            .unwrap()
            .into_iter()
            .filter(|s| s.end_ms > s.start_ms)
            .map(|s| ((s.start_ms - T0) / 1000, (s.end_ms - T0) / 1000, s.app_id))
            .collect();
        spans.sort();
        spans
    }

    #[test]
    fn idle_past_the_threshold_ends_the_span_at_the_last_input() {
        let (_dir, spentday, mut feed) = feed();
        for s in 0..=300 {
            let idle_ms = if s <= 60 { 0 } else { (s - 60) as u64 * 1000 };
            feed.poll(
                T0 + s * 1000,
                window("editor"),
                Some(idle_ms),
                sampled(s, |_| State::Absent),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 60, "editor".into())]);
    }

    #[test]
    fn input_after_being_away_starts_a_new_span() {
        let (_dir, spentday, mut feed) = feed();
        for s in 0..=250 {
            feed.poll(
                T0 + s * 1000,
                window("editor"),
                Some(s as u64 * 1000),
                sampled(s, |_| State::Absent),
            );
        }
        for s in 400..=420 {
            feed.poll(
                T0 + s * 1000,
                window("editor"),
                Some(0),
                sampled(s, |_| State::Absent),
            );
        }
        assert_eq!(spans(&spentday), vec![(400, 420, "editor".into())]);
    }

    #[test]
    fn media_in_the_focused_app_keeps_an_idle_user_present_until_it_stops() {
        let (_dir, spentday, mut feed) = feed();
        let playing = |at| {
            if at <= 500 {
                State::Present
            } else {
                State::Absent
            }
        };
        for s in 0..=600 {
            feed.poll(
                T0 + s * 1000,
                window("totem"),
                Some(s as u64 * 1000),
                sampled(s, playing),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 500, "totem".into())]);
    }

    #[test]
    fn a_video_that_ends_before_the_threshold_keeps_its_time() {
        let (_dir, spentday, mut feed) = feed();
        let playing = |at| {
            if at <= 120 {
                State::Present
            } else {
                State::Absent
            }
        };
        for s in 0..=300 {
            feed.poll(
                T0 + s * 1000,
                window("totem"),
                Some(s as u64 * 1000),
                sampled(s, playing),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 120, "totem".into())]);
    }

    #[test]
    fn a_failed_media_check_keeps_what_the_last_check_found() {
        let (_dir, spentday, mut feed) = feed();
        let failed_once = |at| {
            if at == 305 {
                State::Unknown
            } else {
                State::Present
            }
        };
        for s in 0..=600 {
            feed.poll(
                T0 + s * 1000,
                window("totem"),
                Some(s as u64 * 1000),
                sampled(s, failed_once),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 600, "totem".into())]);
    }

    #[test]
    fn one_check_that_finds_nothing_between_ones_that_find_media_loses_no_time() {
        let (_dir, spentday, mut feed) = feed();
        let between_tracks = |at| {
            if at == 305 {
                State::Absent
            } else {
                State::Present
            }
        };
        for s in 0..=400 {
            feed.poll(
                T0 + s * 1000,
                window("totem"),
                Some(s as u64 * 1000),
                sampled(s, between_tracks),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 400, "totem".into())]);
    }

    #[test]
    fn checks_that_cannot_tell_keep_the_user_present_for_30_s_after_the_last_that_found_media() {
        let (_dir, spentday, mut feed) = feed();
        let then_unknown = |at| {
            if at <= 300 {
                State::Present
            } else {
                State::Unknown
            }
        };
        for s in 0..=600 {
            feed.poll(
                T0 + s * 1000,
                window("totem"),
                Some(s as u64 * 1000),
                sampled(s, then_unknown),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 300, "totem".into())]);
    }

    #[test]
    fn a_stalled_presence_thread_keeps_the_user_present_for_30_s_after_its_last_check() {
        let (_dir, spentday, mut feed) = feed();
        for s in 0..=600 {
            feed.poll(
                T0 + s * 1000,
                window("totem"),
                Some(s as u64 * 1000),
                sampled(s.min(300), |_| State::Present),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 300, "totem".into())]);
    }

    #[test]
    fn media_that_plays_again_brings_an_away_user_back() {
        let (_dir, spentday, mut feed) = feed();
        let playing = |at| {
            if (300..400).contains(&at) {
                State::Absent
            } else {
                State::Present
            }
        };
        for s in 0..=600 {
            feed.poll(
                T0 + s * 1000,
                window("totem"),
                Some(s as u64 * 1000),
                sampled(s, playing),
            );
        }
        assert_eq!(
            spans(&spentday),
            vec![(0, 295, "totem".into()), (400, 600, "totem".into())]
        );
    }

    #[test]
    fn without_an_idle_source_the_user_stays_present() {
        let (_dir, spentday, mut feed) = feed();
        for s in 0..=400 {
            feed.poll(
                T0 + s * 1000,
                window("editor"),
                None,
                sampled(s, |_| State::Absent),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 400, "editor".into())]);
    }

    /// Reports another app on every poll, `spentdayslow` and `spentdayslow-b` in turn, so every
    /// poll heartbeats and opens a span that starts at that heartbeat.
    struct EveryPollNew(u32);

    impl Collector for EveryPollNew {
        fn name(&self) -> &'static str {
            "test"
        }

        fn active(&mut self) -> Result<Option<Window>, String> {
            self.0 += 1;
            let app_id = if self.0.is_multiple_of(2) {
                "spentdayslow"
            } else {
                "spentdayslow-b"
            };
            Ok(Some(Window {
                app_id: app_id.into(),
            }))
        }
    }

    struct Active;

    impl Idle for Active {
        fn idle_ms(&mut self) -> Option<u64> {
            Some(0)
        }
    }

    /// An MPRIS player that answers nothing in time.
    struct SlowPlayer;

    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl SlowPlayer {
        #[zbus(property)]
        fn desktop_entry(&self) -> String {
            thread::sleep(crate::platform::linux::dbus::TIMEOUT * 2);
            "spentdayslow".into()
        }
    }

    struct SlowStatus;

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl SlowStatus {
        #[zbus(property)]
        fn playback_status(&self) -> String {
            thread::sleep(crate::platform::linux::dbus::TIMEOUT * 2);
            "Playing".into()
        }
    }

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn slow_players_never_delay_heartbeats() {
        let _players: Vec<_> = (0..6)
            .map(|i| {
                zbus::blocking::connection::Builder::session()
                    .unwrap()
                    .name(format!("org.mpris.MediaPlayer2.spentdayslow.instance{i}"))
                    .unwrap()
                    .serve_at("/org/mpris/MediaPlayer2", SlowPlayer)
                    .unwrap()
                    .serve_at("/org/mpris/MediaPlayer2", SlowStatus)
                    .unwrap()
                    .build()
                    .unwrap()
            })
            .collect();
        let (_dir, spentday, feed) = feed();
        let (events, rx) = mpsc::channel();
        let tracking = thread::spawn(move || {
            track(
                rx,
                feed,
                Some(Box::new(EveryPollNew(0))),
                Box::new(Active),
                Presence::spawn(),
            )
        });
        thread::sleep(Duration::from_secs(12));
        events.send(Event::Quit).unwrap();
        tracking.join().unwrap();

        let mut heartbeats: Vec<i64> = spentday
            .recent_spans(100)
            .unwrap()
            .into_iter()
            .map(|s| s.start_ms)
            .collect();
        heartbeats.sort();
        let gaps: Vec<i64> = heartbeats.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            heartbeats.len() >= 10,
            "{} heartbeats in 12 s: {gaps:?}",
            heartbeats.len()
        );
        assert!(
            gaps.iter().all(|gap| *gap <= 1_500),
            "heartbeat gaps in ms: {gaps:?}"
        );
    }

    /// GNOME's session manager, inhibiting idle until a call fails.
    struct FlakyInhibitor {
        calls: u32,
    }

    #[zbus::interface(name = "org.gnome.SessionManager")]
    impl FlakyInhibitor {
        fn is_inhibited(&mut self, _flags: u32) -> zbus::fdo::Result<bool> {
            self.calls += 1;
            if self.calls == 1 {
                Ok(true)
            } else {
                Err(zbus::fdo::Error::Failed("busy".into()))
            }
        }
    }

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn an_inhibitor_that_fails_after_saying_yes_keeps_the_user_present() {
        let _gnome = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name("org.gnome.SessionManager")
            .unwrap()
            .serve_at("/org/gnome/SessionManager", FlakyInhibitor { calls: 0 })
            .unwrap()
            .build()
            .unwrap();
        let mut media = Media::new();
        let states = [
            media.sweep(None, Duration::from_secs(2)),
            media.sweep(None, Duration::from_secs(2)),
        ];
        assert_eq!(states, [State::Present, State::Unknown]);

        let (_dir, spentday, mut feed) = feed();
        for s in 0..=320 {
            let state = |at| if at < 305 { states[0] } else { states[1] };
            feed.poll(
                T0 + s * 1000,
                window("editor"),
                Some(s as u64 * 1000),
                sampled(s, state),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 320, "editor".into())]);
    }

    /// A video player whose video plays while the flag is set.
    struct Video(Arc<AtomicBool>);

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Video {
        #[zbus(property)]
        fn playback_status(&self) -> String {
            if self.0.load(Ordering::SeqCst) {
                "Playing"
            } else {
                "Paused"
            }
            .into()
        }
    }

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn the_focused_video_stopping_ends_the_users_time_while_another_player_cannot_tell() {
        let builder = || zbus::blocking::connection::Builder::session().unwrap();
        let playing = Arc::new(AtomicBool::new(true));
        let _video = builder()
            .name("org.mpris.MediaPlayer2.spentdayvideo")
            .unwrap()
            .serve_at("/org/mpris/MediaPlayer2", Video(playing.clone()))
            .unwrap()
            .build()
            .unwrap();
        let _music = builder()
            .name("org.mpris.MediaPlayer2.spentdaymusic")
            .unwrap()
            .serve_at("/org/mpris/MediaPlayer2", SlowPlayer)
            .unwrap()
            .serve_at("/org/mpris/MediaPlayer2", Playing)
            .unwrap()
            .build()
            .unwrap();
        let video = media::App {
            app_id: "spentdayvideo",
            desktop_id: None,
        };
        let mut media = Media::new();
        let mut states = vec![media.sweep(Some(&video), Duration::from_secs(2))];
        playing.store(false, Ordering::SeqCst);
        states.push(media.sweep(Some(&video), Duration::from_secs(2)));
        assert_eq!(states, [State::Present, State::Unknown]);

        let (_dir, spentday, mut feed) = feed();
        for s in 0..=600 {
            let state = |at| if at <= 300 { states[0] } else { states[1] };
            feed.poll(
                T0 + s * 1000,
                window("spentdayvideo"),
                Some(s as u64 * 1000),
                sampled(s, state),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 300, "spentdayvideo".into())]);
    }

    /// A player whose bus name does not name its app, as Chrome's `chromium` player does. Its
    /// `DesktopEntry` answers the first `answers` times, then never in time.
    struct NamedByEntry {
        answers: AtomicU32,
    }

    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl NamedByEntry {
        #[zbus(property)]
        fn desktop_entry(&self) -> String {
            if self
                .answers
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_err()
            {
                thread::sleep(crate::platform::linux::dbus::TIMEOUT * 2);
            }
            "spentday-chrome".into()
        }
    }

    struct Playing;

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Playing {
        #[zbus(property)]
        fn playback_status(&self) -> String {
            "Playing".into()
        }
    }

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn a_player_that_stops_naming_its_app_keeps_the_user_present() {
        let player = |answers| {
            zbus::blocking::connection::Builder::session()
                .unwrap()
                .name("org.mpris.MediaPlayer2.spentdaychromium.instance7")
                .unwrap()
                .serve_at(
                    "/org/mpris/MediaPlayer2",
                    NamedByEntry {
                        answers: AtomicU32::new(answers),
                    },
                )
                .unwrap()
                .serve_at("/org/mpris/MediaPlayer2", Playing)
                .unwrap()
                .build()
                .unwrap()
        };
        let chrome = media::App {
            app_id: "spentday-chrome",
            desktop_id: None,
        };
        let mut media = Media::new();
        let first = player(1);
        let mut states = vec![
            media.sweep(Some(&chrome), Duration::from_secs(2)),
            media.sweep(Some(&chrome), Duration::from_secs(2)),
        ];
        drop(first);
        let _restarted = player(0);
        thread::sleep(Duration::from_millis(300));
        states.push(media.sweep(Some(&chrome), Duration::from_secs(2)));
        assert_eq!(states, [State::Present, State::Present, State::Unknown]);

        let (_dir, spentday, mut feed) = feed();
        for s in 0..=320 {
            let state = |at| if at < 305 { State::Present } else { states[2] };
            feed.poll(
                T0 + s * 1000,
                window("spentday-chrome"),
                Some(s as u64 * 1000),
                sampled(s, state),
            );
        }
        assert_eq!(spans(&spentday), vec![(0, 320, "spentday-chrome".into())]);
    }
}
