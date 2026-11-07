//! Seconds since the last keyboard or pointer input.

mod wayland;

use crate::platform::linux::collector::Kind;
use crate::platform::linux::dbus;
use std::time::{Duration, Instant};
use wayland::WaylandIdle;
use x11rb::connection::Connection as _;
use x11rb::protocol::screensaver::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use zbus::blocking::{Connection, Proxy};

pub trait Idle: Send {
    /// `None` when the source cannot tell; the tracker then treats the user as present.
    fn idle_ms(&mut self) -> Option<u64>;
}

/// Waits before each new search for an idle source after the first fails, the last one
/// repeating. A service can be too slow to answer while the session starts.
const RETRY: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(300),
];

/// `threshold` is how long the user must be idle before the tracker cares, which a source that
/// only reports crossing a timeout (`ext-idle-notify-v1`) needs to know up front.
pub fn open(kind: Option<Kind>, threshold: Duration) -> Box<dyn Idle> {
    match find(kind, threshold) {
        Ok(source) => source,
        Err(failed) => {
            eprintln!(
                "spentday: no idle source on this session ({}); time away from the keyboard counts as active until one answers",
                failed.join(", ")
            );
            Box::new(Searching {
                kind,
                threshold,
                failures: 0,
                next: Instant::now() + RETRY[0],
                found: None,
            })
        }
    }
}

fn find(kind: Option<Kind>, threshold: Duration) -> Result<Box<dyn Idle>, Vec<String>> {
    let mut failed = Vec::new();
    if kind == Some(Kind::X11) {
        match XScreenSaver::connect() {
            Ok(source) => return Ok(Box::new(source)),
            Err(e) => failed.push(format!("XScreenSaver: {e}")),
        }
    }
    let wayland = |failed: &mut Vec<String>| {
        WaylandIdle::connect(threshold)
            .map_err(|e| failed.push(format!("ext-idle-notify-v1: {e}")))
            .ok()
    };
    // Sway and Hyprland have no D-Bus idle service of their own.
    if matches!(kind, Some(Kind::Sway | Kind::Hyprland))
        && let Some(source) = wayland(&mut failed)
    {
        return Ok(Box::new(source));
    }
    match dbus::session() {
        Ok(conn) => {
            let mutter = || {
                DBusIdle::probe(
                    &conn,
                    "org.gnome.Mutter.IdleMonitor",
                    "/org/gnome/Mutter/IdleMonitor/Core",
                    "org.gnome.Mutter.IdleMonitor",
                    "GetIdletime",
                )
            };
            let fdo = || {
                DBusIdle::probe(
                    &conn,
                    "org.freedesktop.ScreenSaver",
                    "/org/freedesktop/ScreenSaver",
                    "org.freedesktop.ScreenSaver",
                    "GetSessionIdleTime",
                )
            };
            if let Some(source) = mutter().or_else(fdo) {
                return Ok(Box::new(source));
            }
            failed.push("no D-Bus idle service answered".into());
        }
        Err(e) => failed.push(format!("D-Bus: {e}")),
    }
    if matches!(kind, Some(Kind::Gnome | Kind::Kde))
        && let Some(source) = wayland(&mut failed)
    {
        return Ok(Box::new(source));
    }
    Err(failed)
}

/// No idle source answered at startup. Until one does, the user counts as present.
struct Searching {
    kind: Option<Kind>,
    threshold: Duration,
    failures: usize,
    next: Instant,
    found: Option<Box<dyn Idle>>,
}

impl Idle for Searching {
    fn idle_ms(&mut self) -> Option<u64> {
        if self.found.is_none() && Instant::now() >= self.next {
            match find(self.kind, self.threshold) {
                Ok(source) => {
                    eprintln!("spentday: an idle source answered");
                    self.found = Some(source);
                }
                Err(_) => {
                    self.failures += 1;
                    self.next = Instant::now() + RETRY[self.failures.min(RETRY.len() - 1)];
                }
            }
        }
        self.found.as_mut()?.idle_ms()
    }
}

struct XScreenSaver {
    conn: RustConnection,
    root: u32,
}

impl XScreenSaver {
    fn connect() -> Result<XScreenSaver, String> {
        let (conn, screen) = RustConnection::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen].root;
        let source = XScreenSaver { conn, root };
        source.query().ok_or("query failed")?;
        Ok(source)
    }

    fn query(&self) -> Option<u64> {
        let info = self
            .conn
            .screensaver_query_info(self.root)
            .ok()?
            .reply()
            .ok()?;
        Some(info.ms_since_user_input.into())
    }
}

impl Idle for XScreenSaver {
    fn idle_ms(&mut self) -> Option<u64> {
        self.query()
    }
}

/// Mutter's `GetIdletime` returns `t` milliseconds. KDE's `GetSessionIdleTime` returns `u`
/// milliseconds (KIdleTime), although the freedesktop spec says seconds.
struct DBusIdle {
    proxy: Proxy<'static>,
    method: &'static str,
}

impl DBusIdle {
    fn probe(
        conn: &Connection,
        dest: &str,
        path: &str,
        iface: &str,
        method: &'static str,
    ) -> Option<DBusIdle> {
        let source = DBusIdle {
            proxy: dbus::proxy(conn, dest, path, iface).ok()?,
            method,
        };
        source.query().is_some().then_some(source)
    }

    fn query(&self) -> Option<u64> {
        let reply = self.proxy.call_method(self.method, &()).ok()?;
        let body = reply.body();
        body.deserialize::<u64>()
            .ok()
            .or_else(|| body.deserialize::<u32>().ok().map(u64::from))
    }
}

impl Idle for DBusIdle {
    fn idle_ms(&mut self) -> Option<u64> {
        self.query()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    /// Answers its first call only after `dbus::TIMEOUT`, like a Mutter still starting up.
    struct SlowFirstMutter(AtomicBool);

    #[zbus::interface(name = "org.gnome.Mutter.IdleMonitor")]
    impl SlowFirstMutter {
        fn get_idletime(&self) -> u64 {
            if !self.0.swap(true, Ordering::SeqCst) {
                thread::sleep(dbus::TIMEOUT * 2);
            }
            4242
        }
    }

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn an_idle_service_slow_to_answer_at_startup_is_found_later() {
        let _mutter = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name("org.gnome.Mutter.IdleMonitor")
            .unwrap()
            .serve_at(
                "/org/gnome/Mutter/IdleMonitor/Core",
                SlowFirstMutter(AtomicBool::new(false)),
            )
            .unwrap()
            .build()
            .unwrap();
        let mut idle = open(Some(Kind::Gnome), Duration::from_secs(180));
        assert_eq!(idle.idle_ms(), None);
        thread::sleep(Duration::from_secs(5));
        assert_eq!(idle.idle_ms(), Some(4242));
    }
}
