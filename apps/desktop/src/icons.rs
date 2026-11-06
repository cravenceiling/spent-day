//! App icon colour seeds. A worker finds the icon (`platform::icon_path`), reads it with
//! `rune::app_icon_rgba(path, 32)` and keeps `colors::dominant`. Stub until unit 1.

use crate::colors::Lch;
use std::collections::HashMap;
use std::sync::Mutex;

/// Keyed by `app_key`; `None` once looked up and no icon was found.
#[derive(Default)]
pub struct Seeds(pub Mutex<HashMap<String, Option<Lch>>>);

impl Seeds {
    /// The cached seed, or `None` after starting a lookup that calls `on_ready` (on its worker
    /// thread) when it lands.
    pub fn get_or_spawn(
        &self,
        app_key: &str,
        app_ids: &[String],
        on_ready: impl FnOnce() + Send + 'static,
    ) -> Option<Lch> {
        let _ = (app_ids, on_ready);
        self.0.lock().unwrap().get(app_key).copied().flatten()
    }
}
