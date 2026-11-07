//! Sleep and lock signals from logind, so the open span closes when the screen goes away.

use crate::platform::linux::tracker::Event;
use std::sync::mpsc::Sender;
use std::thread;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

const LOGIN1: &str = "org.freedesktop.login1";

pub fn watch(events: Sender<Event>) {
    let Ok(conn) = crate::platform::linux::dbus::system() else {
        eprintln!("spentday: no system bus, sleep and lock will not close spans");
        return;
    };
    let sleep_conn = conn.clone();
    let sleep_events = events.clone();
    thread::spawn(move || {
        if let Err(e) = watch_sleep(&sleep_conn, &sleep_events) {
            eprintln!("spentday: not watching sleep: {e}");
        }
    });
    thread::spawn(move || {
        if let Err(e) = watch_lock(&conn, &events) {
            eprintln!("spentday: not watching screen lock: {e}");
        }
    });
}

fn watch_sleep(conn: &Connection, events: &Sender<Event>) -> zbus::Result<()> {
    let manager = Proxy::new(
        conn,
        LOGIN1,
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    for signal in manager.receive_signal("PrepareForSleep")? {
        let going: bool = signal.body().deserialize()?;
        if events.send(Event::Sleep(going)).is_err() {
            break;
        }
    }
    Ok(())
}

fn watch_lock(conn: &Connection, events: &Sender<Event>) -> zbus::Result<()> {
    let session = Proxy::new(
        conn,
        LOGIN1,
        session_path(conn)?,
        "org.freedesktop.login1.Session",
    )?;
    for change in session.receive_property_changed::<bool>("LockedHint") {
        if events.send(Event::Locked(change.get()?)).is_err() {
            break;
        }
    }
    Ok(())
}

/// Our own session, or the user's graphical session when running as a `systemd --user`
/// service, which belongs to no session.
fn session_path(conn: &Connection) -> zbus::Result<OwnedObjectPath> {
    let manager = Proxy::new(
        conn,
        LOGIN1,
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    if let Ok(path) = manager.call("GetSessionByPID", &(std::process::id())) {
        return Ok(path);
    }
    let user: OwnedObjectPath = manager.call("GetUser", &(current_uid()))?;
    let user = Proxy::new(conn, LOGIN1, user, "org.freedesktop.login1.User")?;
    let (_id, path): (String, OwnedObjectPath) = user.get_property("Display")?;
    Ok(path)
}

fn current_uid() -> u32 {
    std::fs::metadata("/proc/self")
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
        .unwrap_or(0)
}
