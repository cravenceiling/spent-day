//! The sync engine: one `SyncState` in `Shared`, copies pushed to the windows. Stubs until unit 3.

use crate::app::Shared;
use jiff::Timestamp;
use spentday_core::SyncReport;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Manual,
    Auto,
}

#[derive(Default)]
pub struct SyncState {
    pub running: bool,
    /// A config change arrived while a sync ran: sync once more with the new account.
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

/// What a window shows of `SyncState`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncView {
    pub running: bool,
    pub configured: bool,
    pub last: Option<LastSync>,
}

/// Starts a sync unless one runs. Off the event loop; reloads the dashboard when it ends.
pub fn run(shared: &Arc<Shared>, trigger: Trigger) {
    let _ = (shared, trigger);
}

pub fn configure(
    shared: &Arc<Shared>,
    server: String,
    code: String,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    let _ = (shared, server, code);
    done(Ok(()));
}

/// A new sync code, then `configure` with it.
pub fn create(
    shared: &Arc<Shared>,
    server: String,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    let _ = (shared, server);
    done(Ok(()));
}

/// Clears the config only when that succeeds.
pub fn turn_off(shared: &Arc<Shared>, done: impl FnOnce(Result<(), String>) + Send + 'static) {
    let _ = shared;
    done(Ok(()));
}

/// Pushes a `SyncView` to the dashboard (its settings overlay) and the panel.
pub fn broadcast(shared: &Arc<Shared>) {
    let _ = shared;
}
