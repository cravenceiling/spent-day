mod autostart;
mod collector;
mod dbus;
mod desktop;
mod idle;
mod instance;
mod media;
mod paths;
mod session;
mod tracker;

use super::{OnFatal, Pref};
pub use autostart::{autostart, set_autostart};
pub use instance::{InstanceGuard, claim_instance};
pub use paths::db_path;
use spentday_core::SpentDay;
use std::path::PathBuf;
use std::sync::Arc;

pub const PLATFORM: spentday_core::Platform = spentday_core::Platform::Linux;
pub const DEVICE_NOUN: &str = "computer";

pub fn device_name() -> String {
    let mut bytes = [0u8; 256];
    // Linux limits hostnames to 64 bytes; the buffer also leaves room for the terminator.
    let result = unsafe { libc::gethostname(bytes.as_mut_ptr().cast(), bytes.len()) };
    if result == 0 {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let name = String::from_utf8_lossy(&bytes[..end]);
        if !name.is_empty() {
            return name.into_owned();
        }
    }
    "computer".into()
}

pub struct Tracking(tracker::Tracker);

pub fn start_tracking(core: Arc<SpentDay>, on_fatal: OnFatal) -> Tracking {
    let (tracker, started) = tracker::Tracker::spawn(core);
    std::thread::spawn(move || match started.recv() {
        Ok(Err(error)) => on_fatal(error),
        Err(_) => on_fatal("The tracking thread stopped before collection could start.".into()),
        Ok(Ok(())) => {}
    });
    Tracking(tracker)
}

impl Tracking {
    pub fn stop(self) {
        self.0.stop();
    }
}

pub fn icon_path(app_key: &str, app_ids: &[String]) -> Option<PathBuf> {
    let mut entries = desktop::Entries::new();
    app_ids
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(app_key))
        .find_map(|id| entries.path(id))
}

pub fn read_pref(_pref: Pref) -> Option<bool> {
    None
}

pub fn write_pref(_pref: Pref, _on: bool) {}

pub fn install_quit_handler(_quit: Box<dyn Fn()>) {}
