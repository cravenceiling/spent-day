//! The platform seam on macOS. Stubs until unit 4.

use super::{Instance, OnActivate, OnFatal, Pref};
use spentday_core::SpentDay;
use std::path::PathBuf;
use std::sync::Arc;

pub const PLATFORM: spentday_core::Platform = spentday_core::Platform::Macos;
/// Fills "this Mac" in user-facing text.
pub const DEVICE_NOUN: &str = "Mac";

/// `SPENTDAY_DB` verbatim, else `<data>/spentday.db` in a folder made and kept 0700. Refuses
/// a non-UTF-8 path. The stub never touches the real data folder.
pub fn db_path() -> Result<String, String> {
    std::env::var("SPENTDAY_DB").map_err(|_| "Spent Day can't find its data folder yet".to_string())
}

/// Off the event loop.
pub fn device_name() -> String {
    DEVICE_NOUN.to_string()
}

pub struct Tracking {}

/// Starts collection on its own threads.
pub fn start_tracking(core: Arc<SpentDay>, on_fatal: OnFatal) -> Tracking {
    let _ = (core, on_fatal);
    Tracking {}
}

impl Tracking {
    /// Flushes the open span; bounded at 3 s.
    pub fn stop(self) {}
}

/// The installed app's icon file (`.app` bundle or `.desktop` file); the first installed id wins.
pub fn icon_path(app_key: &str, app_ids: &[String]) -> Option<PathBuf> {
    let _ = (app_key, app_ids);
    None
}

pub struct InstanceGuard {}

/// macOS: always Primary; LaunchServices and `on_reopen` cover second launches.
pub fn claim_instance(demo: bool, show: bool, on_activate: OnActivate) -> Instance {
    let _ = (demo, show, on_activate);
    Instance::Primary(InstanceGuard {})
}

/// `None`: the platform has no such setting, so its row or sheet is hidden.
pub fn read_pref(p: Pref) -> Option<bool> {
    let _ = p;
    None
}

pub fn write_pref(p: Pref, on: bool) {
    let _ = (p, on);
}

/// `None`: no launch-at-login toggle.
pub fn autostart() -> Option<bool> {
    None
}

/// SMAppService. `Ok(Some(note))` asks the user to allow it in System Settings.
pub fn set_autostart(on: bool) -> Result<Option<String>, String> {
    let _ = on;
    Err("not supported".into())
}

/// Main thread, after the loop starts. Installs the status item; `on_click` gets its button rect in top-left global points.
pub fn menu_bar(on_click: Box<dyn Fn([f64; 4])>) {
    let _ = on_click;
}

/// An `aevt/quit` handler, so logout and AppleScript quit take the quit path.
pub fn install_quit_handler(quit: Box<dyn Fn()>) {
    let _ = quit;
}
