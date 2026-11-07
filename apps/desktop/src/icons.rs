//! Icon sampling stays on a worker; both found and missing seeds are cached by app key.

use crate::colors::{self, Lch, Rgb};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// `None` also reserves an in-flight lookup, so repeated frames do not spawn more workers.
#[derive(Default)]
pub struct Seeds(pub Arc<Mutex<HashMap<String, Option<Lch>>>>);

impl Seeds {
    /// Calls `on_ready` on the lookup worker after storing its result.
    pub fn get_or_spawn(
        &self,
        app_key: &str,
        app_ids: &[String],
        on_ready: impl FnOnce() + Send + 'static,
    ) -> Option<Lch> {
        let mut cache = self.0.lock().unwrap();
        if let Some(seed) = cache.get(app_key) {
            return *seed;
        }
        cache.insert(app_key.to_owned(), None);
        drop(cache);
        let (cache, key, ids) = (self.0.clone(), app_key.to_owned(), app_ids.to_vec());
        std::thread::spawn(move || {
            let seed = crate::platform::icon_path(&key, &ids)
                .and_then(|path| rune::image::app_icon_rgba(&path, 32).ok())
                .map(|(_, _, rgba)| {
                    colors::dominant(rgba.as_chunks::<4>().0.iter().map(|p| {
                        (
                            Rgb {
                                r: f64::from(p[0]) / 255.,
                                g: f64::from(p[1]) / 255.,
                                b: f64::from(p[2]) / 255.,
                            },
                            f64::from(p[3]) / 255.,
                        )
                    }))
                    .unwrap_or(Lch {
                        l: 0.37,
                        c: 0.,
                        h: 0.,
                    })
                });
            cache.lock().unwrap().insert(key, seed);
            on_ready();
        });
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn missing_icon_is_cached_and_callback_runs_on_worker() {
        let seeds = Seeds::default();
        let caller = std::thread::current().id();
        let (tx, rx) = mpsc::channel();
        let key = "spentday.unit1.nonexistent-app-684913";
        assert_eq!(
            seeds.get_or_spawn(key, &[], move || {
                tx.send(std::thread::current().id()).unwrap();
            }),
            None
        );
        assert_ne!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), caller);
        assert_eq!(seeds.0.lock().unwrap().get(key), Some(&None));
        assert_eq!(
            seeds.get_or_spawn(key, &[], || panic!("cached lookup spawned again")),
            None
        );
    }

    #[test]
    fn cached_color_returns_without_lookup() {
        let seeds = Seeds::default();
        let seed = colors::hashed("app:example");
        seeds.0.lock().unwrap().insert("example".into(), Some(seed));
        assert_eq!(
            seeds.get_or_spawn("example", &[], || panic!("cached lookup spawned again")),
            Some(seed)
        );
    }
}
