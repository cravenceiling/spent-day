//! What keeps an idle user present: an idle inhibitor, which video players and browsers take
//! while a video plays, or an MPRIS player playing in the app in front. Music playing in
//! another app while the user is away does not count.
//!
//! A slow or hung player must never hold up focus tracking, so the checks run on their own
//! thread, which publishes what it found as a `Snapshot` the tracker reads.

use crate::platform::linux::dbus;
use crate::platform::linux::desktop::Entries;
use crate::platform::linux::tracker::now_ms;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, MessageIterator, fdo::DBusProxy};
use zbus::{DBusError, MatchRule, message};

const MPRIS: &str = "org.mpris.MediaPlayer2.";
/// `GsmInhibitorFlag` for idle in `org.gnome.SessionManager.IsInhibited`.
const GNOME_INHIBIT_IDLE: u32 = 8;
const EVERY: Duration = Duration::from_secs(5);
/// How long one round of checks may take. Sources not reached by then count as unknown.
const BUDGET: Duration = Duration::from_secs(2);

/// Ordered so that combining sources is `max`: any source that finds something keeping the
/// user present wins, and otherwise one that could not tell makes the whole answer unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Absent,
    Unknown,
    Present,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub state: State,
    /// When the round of checks that found `state` started.
    pub observed_at_ms: i64,
}

/// The presence thread. Dropping this stops it after its current round.
pub struct Presence {
    focused: Arc<Mutex<Option<String>>>,
    latest: Arc<Mutex<Option<Snapshot>>>,
    _stop: Sender<()>,
}

impl Presence {
    pub fn spawn() -> Presence {
        let focused = Arc::new(Mutex::new(None));
        let latest = Arc::new(Mutex::new(None));
        let (stop, stopped) = mpsc::channel();
        let (app_id, snapshot) = (focused.clone(), latest.clone());
        thread::Builder::new()
            .name("presence".into())
            .spawn(move || {
                let mut media = Media::new();
                let mut entries = Entries::new();
                loop {
                    let observed_at_ms = now_ms();
                    let app_id: Option<String> = app_id.lock().unwrap().clone();
                    let app = app_id.as_deref().map(|app_id| App {
                        app_id,
                        desktop_id: entries.info(app_id).desktop_id.as_deref(),
                    });
                    let state = media.sweep(app.as_ref(), BUDGET);
                    *snapshot.lock().unwrap() = Some(Snapshot {
                        state,
                        observed_at_ms,
                    });
                    if stopped.recv_timeout(EVERY) != Err(RecvTimeoutError::Timeout) {
                        break;
                    }
                }
            })
            .expect("spawn presence thread");
        Presence {
            focused,
            latest,
            _stop: stop,
        }
    }

    /// The app in front, by its app id, for the next round of checks.
    pub fn focus(&self, app_id: Option<&str>) {
        *self.focused.lock().unwrap() = app_id.map(str::to_owned);
    }

    /// `None` until the first round of checks finishes.
    pub fn latest(&self) -> Option<Snapshot> {
        *self.latest.lock().unwrap()
    }
}

/// The focused app, by the names an MPRIS player may use for it.
pub struct App<'a> {
    pub app_id: &'a str,
    /// Its `.desktop` file name without the extension.
    pub desktop_id: Option<&'a str>,
}

/// Combines sources as they are checked, and stops asking once one finds the user present or
/// the round runs out of time.
struct Round {
    state: State,
    deadline: Instant,
}

impl Round {
    fn check(&mut self, source: impl FnOnce() -> State) {
        if self.state == State::Present {
            return;
        }
        let found = if Instant::now() < self.deadline {
            source()
        } else {
            State::Unknown
        };
        self.state = self.state.max(found);
    }
}

pub struct Media {
    session: Option<Connection>,
    system: Option<Connection>,
    /// Each player's `DesktopEntry` by bus name, `None` for a player that has none.
    entries: HashMap<String, Option<String>>,
    /// Player bus names whose owner changed, so their cached entry may be another player's.
    /// `None` when not watching, and then nothing stays cached.
    owner_changes: Option<Receiver<String>>,
}

impl Media {
    pub fn new() -> Media {
        let session = dbus::session().ok();
        let owner_changes = session.as_ref().and_then(|conn| {
            watch_owners(conn)
                .map_err(|e| eprintln!("spentday: not watching MPRIS players come and go: {e}"))
                .ok()
        });
        Media {
            session,
            system: dbus::system().ok(),
            entries: HashMap::new(),
            owner_changes,
        }
    }

    /// One round of checks for the focused app, given at most `budget`.
    pub fn sweep(&mut self, focused: Option<&App>, budget: Duration) -> State {
        let mut round = Round {
            state: State::Absent,
            deadline: Instant::now() + budget,
        };
        round.check(|| self.logind_inhibited());
        round.check(|| self.gnome_inhibited());
        let Some(app) = focused else {
            return round.state;
        };
        self.forget_replaced_players();
        let mut players = Vec::new();
        round.check(|| match self.players() {
            Ok(names) => {
                players = names;
                State::Absent
            }
            Err(state) => state,
        });
        // Players named after the focused app go first, so a slow stranger cannot use up the budget.
        players.sort_by_key(|name| !belongs_to(name, None, app));
        for name in &players {
            round.check(|| self.playing_in(name, app));
        }
        round.state
    }

    fn logind_inhibited(&self) -> State {
        let Some(conn) = &self.system else {
            return State::Absent;
        };
        answer(
            dbus::proxy(
                conn,
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
            )
            .and_then(|p| p.get_property::<String>("BlockInhibited")),
            |what| blocks_idle(&what),
        )
    }

    fn gnome_inhibited(&self) -> State {
        let Some(conn) = &self.session else {
            return State::Absent;
        };
        answer(
            dbus::proxy(
                conn,
                "org.gnome.SessionManager",
                "/org/gnome/SessionManager",
                "org.gnome.SessionManager",
            )
            .and_then(|p| p.call::<_, _, bool>("IsInhibited", &(GNOME_INHIBIT_IDLE))),
            |inhibited| inhibited,
        )
    }

    /// The bus names of the MPRIS players on the session bus.
    fn players(&self) -> Result<Vec<String>, State> {
        let Some(conn) = &self.session else {
            return Ok(Vec::new());
        };
        let names = DBusProxy::new(conn)
            .and_then(|bus| Ok(bus.list_names()?))
            .map_err(|e| classify(&e))?;
        Ok(names
            .into_iter()
            .map(|name| name.to_string())
            .filter(|name| name.starts_with(MPRIS))
            .collect())
    }

    fn playing_in(&mut self, name: &str, app: &App) -> State {
        let owned = if belongs_to(name, None, app) {
            Ok(true)
        } else {
            self.desktop_entry(name)
                .map(|entry| belongs_to(name, entry.as_deref(), app))
        };
        match owned {
            Ok(true) => self.playing(name),
            Ok(false) => State::Absent,
            // A player that will not say whose it is might be the focused app's.
            Err(()) => self.playing(name).min(State::Unknown),
        }
    }

    fn playing(&self, name: &str) -> State {
        let Some(conn) = &self.session else {
            return State::Absent;
        };
        answer(
            dbus::proxy(
                conn,
                name,
                "/org/mpris/MediaPlayer2",
                "org.mpris.MediaPlayer2.Player",
            )
            .and_then(|p| p.get_property::<String>("PlaybackStatus")),
            |status| status == "Playing",
        )
    }

    /// `Err` when the player did not answer and no earlier answer is cached.
    fn desktop_entry(&mut self, name: &str) -> Result<Option<String>, ()> {
        if let Some(entry) = self.entries.get(name) {
            return Ok(entry.clone());
        }
        let Some(conn) = &self.session else {
            return Ok(None);
        };
        let entry = match dbus::proxy(
            conn,
            name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2",
        )
        .and_then(|p| p.get_property::<String>("DesktopEntry"))
        {
            Ok(entry) => Some(entry),
            Err(e) if classify(&e) == State::Absent => None,
            Err(_) => return Err(()),
        };
        if self.owner_changes.is_some() {
            self.entries.insert(name.to_owned(), entry.clone());
        }
        Ok(entry)
    }

    fn forget_replaced_players(&mut self) {
        let Some(changes) = &self.owner_changes else {
            return;
        };
        loop {
            match changes.try_recv() {
                Ok(name) => {
                    self.entries.remove(&name);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.owner_changes = None;
                    self.entries.clear();
                    return;
                }
            }
        }
    }
}

/// Sends the bus name of every MPRIS player that appears, goes away or changes owner.
fn watch_owners(conn: &Connection) -> zbus::Result<Receiver<String>> {
    let rule = MatchRule::builder()
        .msg_type(message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns("org.mpris.MediaPlayer2")?
        .build();
    let signals = MessageIterator::for_match_rule(rule, conn, Some(64))?;
    let (changed, changes) = mpsc::channel();
    thread::Builder::new()
        .name("mpris-owners".into())
        .spawn(move || {
            for signal in signals.flatten() {
                let Ok((name, _, _)) = signal.body().deserialize::<(String, String, String)>()
                else {
                    continue;
                };
                if changed.send(name).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| zbus::Error::InputOutput(Arc::new(e)))?;
    Ok(changes)
}

fn answer<T>(result: zbus::Result<T>, found: impl FnOnce(T) -> bool) -> State {
    match result.map(found) {
        Ok(true) => State::Present,
        Ok(false) => State::Absent,
        Err(e) => classify(&e),
    }
}

/// Error replies that say the source is not there, so it holds nothing. `UnknownProperty` and
/// `InvalidArgs` are how zbus and GDBus answer for a property the object does not have.
const ABSENT: [&str; 8] = [
    "org.freedesktop.DBus.Error.ServiceUnknown",
    "org.freedesktop.DBus.Error.NameHasNoOwner",
    "org.freedesktop.DBus.Error.UnknownMethod",
    "org.freedesktop.DBus.Error.UnknownInterface",
    "org.freedesktop.DBus.Error.UnknownObject",
    "org.freedesktop.DBus.Error.NotSupported",
    "org.freedesktop.DBus.Error.UnknownProperty",
    "org.freedesktop.DBus.Error.InvalidArgs",
];

/// Any other error, such as `Failed`, `NoReply`, a timeout or a lost connection, says nothing
/// about the source.
fn classify(e: &zbus::Error) -> State {
    let absent = match e {
        zbus::Error::MethodError(name, ..) => ABSENT.contains(&name.as_str()),
        zbus::Error::FDO(e) => ABSENT.contains(&e.name().as_str()),
        _ => false,
    };
    if absent {
        State::Absent
    } else {
        State::Unknown
    }
}

/// logind's `BlockInhibited` is a colon-separated list such as `handle-power-key:idle:sleep`.
fn blocks_idle(what: &str) -> bool {
    what.split(':').any(|w| w == "idle")
}

/// A player names its app by its bus name (`org.mpris.MediaPlayer2.firefox.instance_1_84`) and
/// its `DesktopEntry` (`google-chrome`, `org.gnome.Totem`).
fn belongs_to(bus_name: &str, desktop_entry: Option<&str>, app: &App) -> bool {
    let player = bus_name.strip_prefix(MPRIS).unwrap_or(bus_name);
    let player = player
        .split_once(".instance")
        .map_or(player, |(name, _)| name);
    let players = [Some(player), desktop_entry];
    let apps = [Some(app.app_id), app.desktop_id];
    players
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .any(|p| {
            apps.into_iter()
                .flatten()
                .any(|a| a.eq_ignore_ascii_case(p))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app<'a>(app_id: &'a str, desktop_id: Option<&'a str>) -> App<'a> {
        App { app_id, desktop_id }
    }

    struct Root;

    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl Root {
        #[zbus(property)]
        fn desktop_entry(&self) -> String {
            "spentdaytest".into()
        }
    }

    struct Player;

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Player {
        #[zbus(property)]
        fn playback_status(&self) -> String {
            "Playing".into()
        }
    }

    /// Answers after `dbus::TIMEOUT`.
    struct SlowPlayer;

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl SlowPlayer {
        #[zbus(property)]
        fn playback_status(&self) -> String {
            std::thread::sleep(dbus::TIMEOUT * 2);
            "Playing".into()
        }
    }

    struct SessionManager;

    #[zbus::interface(name = "org.gnome.SessionManager")]
    impl SessionManager {
        fn is_inhibited(&self, flags: u32) -> bool {
            flags == GNOME_INHIBIT_IDLE
        }
    }

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn reads_players_and_inhibitors_over_dbus() {
        let mut media = Media::new();
        let builder = || zbus::blocking::connection::Builder::session().unwrap();
        let _player = builder()
            .name("org.mpris.MediaPlayer2.spentdaytest.instance42")
            .unwrap()
            .serve_at("/org/mpris/MediaPlayer2", Root)
            .unwrap()
            .serve_at("/org/mpris/MediaPlayer2", Player)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            media.sweep(Some(&app("spentdaytest", None)), BUDGET),
            State::Present
        );
        assert_eq!(
            media.sweep(Some(&app("firefox", Some("firefox"))), BUDGET),
            State::Absent
        );
        assert_eq!(media.sweep(None, BUDGET), State::Absent);

        let _slow = builder()
            .name("org.mpris.MediaPlayer2.spentdaytestslow")
            .unwrap()
            .serve_at("/org/mpris/MediaPlayer2", SlowPlayer)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            media.sweep(Some(&app("spentdaytestslow", None)), BUDGET),
            State::Unknown
        );
        assert_eq!(
            media.sweep(Some(&app("spentdaytest", None)), BUDGET),
            State::Present
        );

        let _gnome = builder()
            .name("org.gnome.SessionManager")
            .unwrap()
            .serve_at("/org/gnome/SessionManager", SessionManager)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(media.sweep(None, BUDGET), State::Present);
    }

    #[test]
    fn idle_in_the_inhibited_list() {
        assert!(blocks_idle("handle-power-key:idle:sleep"));
        assert!(blocks_idle("idle"));
        assert!(!blocks_idle("handle-power-key:handle-suspend-key:sleep"));
        assert!(!blocks_idle(""));
    }

    #[test]
    fn players_belong_to_the_app_they_name() {
        let firefox = "org.mpris.MediaPlayer2.firefox.instance_1_84";
        assert!(belongs_to(
            firefox,
            Some("firefox"),
            &app("firefox", Some("firefox"))
        ));
        assert!(belongs_to(firefox, None, &app("firefox", None)));
        assert!(!belongs_to(
            firefox,
            Some("firefox"),
            &app("code", Some("code"))
        ));

        let chrome = "org.mpris.MediaPlayer2.chromium.instance4123";
        assert!(belongs_to(
            chrome,
            Some("google-chrome"),
            &app("google-chrome", Some("google-chrome"))
        ));
        assert!(!belongs_to(
            chrome,
            Some("google-chrome"),
            &app("org.gnome.Ptyxis", None)
        ));

        assert!(belongs_to(
            "org.mpris.MediaPlayer2.spotify",
            Some(""),
            &app("Spotify", None)
        ));
        assert!(belongs_to(
            "org.mpris.MediaPlayer2.totem",
            Some("org.gnome.Totem"),
            &app("Totem", Some("org.gnome.Totem"))
        ));
        assert!(!belongs_to(
            "org.mpris.MediaPlayer2.spotify",
            Some("spotify"),
            &app("firefox", Some("firefox"))
        ));
    }
}
