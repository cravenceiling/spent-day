//! `ext-idle-notify-v1`, which Sway, Hyprland and KWin offer to any client. The compositor
//! says when the user has been idle for the threshold and when input resumes. It holds the
//! notice back while an app inhibits idle, such as a browser playing a video.

use super::Idle;
use std::os::fd::{AsFd, AsRawFd};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

/// The last input, once the compositor has said the user is idle.
type LastInput = Arc<Mutex<Option<Instant>>>;

pub struct WaylandIdle {
    last_input: LastInput,
}

struct State {
    last_input: LastInput,
    threshold: Duration,
}

/// How long the compositor may take to answer the setup round trips.
const SETUP_TIMEOUT: Duration = Duration::from_secs(2);

impl WaylandIdle {
    pub fn connect(threshold: Duration) -> Result<WaylandIdle, String> {
        WaylandIdle::start(
            Connection::connect_to_env().map_err(|e| e.to_string())?,
            threshold,
        )
    }

    /// The setup round trips block, so they run on the thread that dispatches afterwards, and
    /// this one stops waiting at `SETUP_TIMEOUT`. The rendezvous channel tells that thread
    /// whether anyone still waits for it.
    fn start(conn: Connection, threshold: Duration) -> Result<WaylandIdle, String> {
        let last_input = LastInput::default();
        let mut state = State {
            last_input: last_input.clone(),
            threshold,
        };
        let (ready, setup) = mpsc::sync_channel(0);
        let dispatch = conn.clone();
        thread::Builder::new()
            .name("wayland-idle".into())
            .spawn(move || {
                let (mut queue, objects) = match subscribe(&dispatch, &mut state) {
                    Ok(subscribed) => subscribed,
                    Err(e) => return drop(ready.send(Err(e))),
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                let _objects = objects;
                while queue.blocking_dispatch(&mut state).is_ok() {}
                eprintln!("spentday: lost the Wayland connection; time away from the keyboard counts as active");
                *state.last_input.lock().unwrap() = None;
            })
            .map_err(|e| e.to_string())?;
        match setup.recv_timeout(SETUP_TIMEOUT) {
            Ok(result) => result.map(|()| WaylandIdle { last_input }),
            Err(_) => {
                // Wakes the setup thread, so it ends instead of waiting on the compositor forever.
                // SAFETY: shutdown(2) takes no pointers, and `conn` keeps the socket open.
                unsafe { libc::shutdown(conn.as_fd().as_raw_fd(), libc::SHUT_RDWR) };
                Err(format!(
                    "the compositor did not answer within {} s",
                    SETUP_TIMEOUT.as_secs()
                ))
            }
        }
    }
}

type Objects = (wl_seat::WlSeat, ExtIdleNotifierV1, ExtIdleNotificationV1);

fn subscribe(conn: &Connection, state: &mut State) -> Result<(EventQueue<State>, Objects), String> {
    let (globals, mut queue) = registry_queue_init::<State>(conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("wl_seat: {e}"))?;
    let notifier: ExtIdleNotifierV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("ext_idle_notifier_v1: {e}"))?;
    let millis = u32::try_from(state.threshold.as_millis()).unwrap_or(u32::MAX);
    let notification = notifier.get_idle_notification(millis, &seat, &qh, ());
    queue.roundtrip(state).map_err(|e| e.to_string())?;
    Ok((queue, (seat, notifier, notification)))
}

impl Idle for WaylandIdle {
    fn idle_ms(&mut self) -> Option<u64> {
        let last_input = *self.last_input.lock().unwrap();
        Some(last_input.map_or(0, |at| at.elapsed().as_millis() as u64))
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let now = Instant::now();
        *state.last_input.lock().unwrap() = match event {
            ext_idle_notification_v1::Event::Idled => {
                Some(now.checked_sub(state.threshold).unwrap_or(now))
            }
            ext_idle_notification_v1::Event::Resumed => None,
            _ => return,
        };
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ExtIdleNotifierV1);

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn a_compositor_that_never_answers_fails_within_the_deadline() {
        let (ours, _silent) = UnixStream::pair().unwrap();
        let conn = Connection::from_socket(ours).unwrap();
        let started = Instant::now();
        let (done, result) = mpsc::channel();
        thread::spawn(move || done.send(WaylandIdle::start(conn, Duration::from_secs(180)).err()));
        let result = result
            .recv_timeout(Duration::from_secs(4))
            .unwrap_or(Some("still waiting after 4 s".into()));
        assert_eq!(
            result,
            Some("the compositor did not answer within 2 s".into())
        );
        assert!(
            started.elapsed() < Duration::from_millis(2500),
            "took {:?}",
            started.elapsed()
        );
    }
}
