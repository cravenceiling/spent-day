//! Blocking core work never runs on the event loop: it runs on its own thread and its result
//! comes back through the window's `AppHandle`.

use rune::{AppHandle, Tx};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `work` on a new thread, then `done` on the UI thread with its result. `done` returns
/// whether it changed what the view reads.
pub fn spawn<S: 'static, T: Send + 'static>(
    to: AppHandle<S>,
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(&mut S, T, &mut Tx) -> bool + Send + 'static,
) {
    std::thread::spawn(move || match catch_unwind(AssertUnwindSafe(work)) {
        Ok(out) => to.update_if(move |s, tx| done(s, out, tx)),
        Err(_) => eprintln!("spentday: background task panicked"),
    });
}
