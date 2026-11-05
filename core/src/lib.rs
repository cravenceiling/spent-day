//! Public contract shared by every Spent Day app. Swift and Kotlin see exactly this
//! through UniFFI; the Linux app links it as a Rust crate. See docs/DESIGN.md.

use std::sync::{Arc, Mutex, MutexGuard};

mod apps;
mod day;
pub use day::{DayDetail, DayItem, DayQuery, DeviceDay, ItemKind, Session};
mod listener;
mod store;
mod summary;
mod sync;
mod tracker;
mod web;

pub use store::Span;

uniffi::setup_scaffolding!();

pub type Millis = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Linux,
    Android,
    Ios,
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub last_seen_ms: Millis,
}

/// The page a browser shows. The core keeps only the domain of `url` and
/// never stores the URL itself, so a path, query or fragment stays in memory.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct WebPage {
    /// A full URL or just an origin (`https://www.youtube.com`). Schemeless
    /// input (`youtube.com/watch`, as Android URL bars show it) reads as https.
    pub url: String,
}

/// What a collector sees in front of the user right now.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct Activity {
    /// Platform app id: bundle id, Android package, or X11/Wayland app class.
    pub app_id: String,
    pub app_name: String,
    /// Set when the collector read the URL itself (macOS AppleScript).
    /// When `None` and the app is a browser, the core attributes the latest web hint.
    pub web: Option<WebPage>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct SummaryQuery {
    pub start_ms: Millis,
    pub end_ms: Millis,
    /// Width of each slot in `Summary::buckets`. 3_600_000 for an hourly day view.
    pub bucket_ms: Millis,
    /// `None` means all devices.
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct DeviceUsage {
    pub device: DeviceInfo,
    pub total_ms: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct DomainUsage {
    pub domain: String,
    pub total_ms: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct AppUsage {
    /// Canonical key: same value for Chrome on macOS, Linux and Android.
    pub app_key: String,
    pub app_name: String,
    pub is_browser: bool,
    pub total_ms: Millis,
    /// Sorted by time, descending. Empty for non-browsers. Browser time with no
    /// site recorded is one row with an empty `domain`, which apps show as "Other".
    pub domains: Vec<DomainUsage>,
    /// The distinct platform `app_id`s behind this row, most time first, ties
    /// by id. Apps use them to find a local icon (`com.google.Chrome` for `chrome`).
    #[uniffi(default = [])]
    pub app_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct Summary {
    pub start_ms: Millis,
    pub end_ms: Millis,
    pub total_ms: Millis,
    pub by_device: Vec<DeviceUsage>,
    /// Sorted by time, descending.
    pub apps: Vec<AppUsage>,
    /// Domains across all browsers and devices, sorted by time, descending.
    /// Never has an empty `domain`.
    pub sites: Vec<DomainUsage>,
    pub buckets: Vec<Millis>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct SyncConfig {
    pub server_url: String,
    pub sync_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record, serde::Serialize)]
pub struct SyncReport {
    pub pushed_spans: u64,
    pub pulled_spans: u64,
    pub finished_at_ms: Millis,
    /// Batches from the server that would not decrypt or parse. Sync skips
    /// them for good, so one corrupt batch cannot stop every later one.
    #[uniffi(default = 0)]
    pub skipped_batches: u64,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SpentDayError {
    #[error("storage: {0}")]
    Storage(String),
    #[error("sync is not configured")]
    SyncNotConfigured,
    #[error("invalid sync code")]
    InvalidSyncCode,
    #[error("network: {0}")]
    Network(String),
    #[error("server rejected request: {status} {message}")]
    Server { status: u16, message: String },
    #[error("could not decrypt data from the server: wrong sync code?")]
    Decrypt,
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// The database at `path` is from an early build that stored web
    /// addresses and titles. There is no migration, so the only way on is to
    /// delete it. Apps show their own "start over" steps for this one error.
    #[error("{path} was made by an early test build of Spent Day and can't be opened. Delete it, with its -wal and -shm files, to start over.")]
    OldDatabase { path: String },
}

pub(crate) fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as Millis)
}

fn own_device(store: &store::Store) -> DeviceInfo {
    let devices = store.devices().unwrap_or_default();
    devices
        .into_iter()
        .find(|d| d.id == store.device_id)
        .unwrap_or_else(|| DeviceInfo {
            id: store.device_id.clone(),
            name: String::new(),
            platform: Platform::Linux,
            last_seen_ms: 0,
        })
}

/// State shared with the web listener thread.
pub(crate) struct Inner {
    store: Mutex<store::Store>,
}

impl Inner {
    fn store(&self) -> MutexGuard<'_, store::Store> {
        self.store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn device_name(&self) -> String {
        own_device(&self.store()).name
    }

    /// `None` records a no-page hint.
    pub(crate) fn web_hint(
        &self,
        at_ms: Millis,
        browser: &str,
        page: Option<&WebPage>,
    ) -> Result<(), SpentDayError> {
        web::record_hint(&self.store(), at_ms, browser, page)
    }
}

/// One per process. Thread-safe; every method may block on SQLite or the network,
/// so UI code calls it off the main thread.
#[derive(uniffi::Object)]
pub struct SpentDay {
    inner: Arc<Inner>,
    listener_started: Mutex<bool>,
    /// Held for a whole `sync_now`, so the UI and a background job never
    /// push the same batch twice.
    syncing: Mutex<()>,
}

impl SpentDay {
    fn from_store(store: store::Store) -> Result<Arc<Self>, SpentDayError> {
        web::prune(&store)?;
        Ok(Arc::new(SpentDay {
            inner: Arc::new(Inner {
                store: Mutex::new(store),
            }),
            listener_started: Mutex::new(false),
            syncing: Mutex::new(()),
        }))
    }

    /// Opens an existing database without touching its device name or platform.
    /// Rust-only, for tools such as `spentday-cli` that inspect another app's data.
    pub fn open_existing(db_path: &str) -> Result<Arc<Self>, SpentDayError> {
        Self::from_store(store::Store::open(db_path, false)?)
    }

    /// Raw span rows from every device, newest first. Rust-only, for `spentday-cli`.
    pub fn recent_spans(&self, limit: usize) -> Result<Vec<Span>, SpentDayError> {
        self.inner.store().recent_spans(limit)
    }

    /// The client for the stored config, and the config generation it belongs to.
    fn sync_client(&self) -> Result<(sync::Client, i64), SpentDayError> {
        let (config, generation) = self.inner.store().sync_config()?;
        let (server_url, code) = config.ok_or(SpentDayError::SyncNotConfigured)?;
        // A URL stored by an older build was never held to today's rule, so
        // every client is built from the URL as it checks out now.
        let server_url = sync::check_server_url(&server_url)?;
        let client = sync::Client::new(&server_url, sync::Keys::derive(&code)?);
        Ok((client, generation))
    }

    /// Stops early, without error, once the sync config changes under it.
    fn push(&self, client: &sync::Client, generation: i64) -> Result<u64, SpentDayError> {
        let mut pushed = 0;
        loop {
            // One snapshot, so a batch read under this generation only holds
            // spans recorded before any account switch.
            let next = self.inner.store().snapshot(|store| {
                if store.meta_i64(store::meta::SYNC_GENERATION)? != generation {
                    return Ok(None);
                }
                let device = own_device(store);
                let device = sync::WireDevice {
                    id: device.id,
                    name: device.name,
                    platform: device.platform,
                };
                let spans = sync::fit_batch(
                    &device,
                    store.own_spans_after_rev(
                        store.meta_i64(store::meta::PUSHED_REV)?,
                        sync::PUSH_BATCH_SPANS,
                    )?,
                );
                let Some(max_rev) = spans.last().map(|s| s.rev) else {
                    return Ok(None);
                };
                Ok(Some((sync::Batch { device, spans }, max_rev)))
            })?;
            let Some((batch, max_rev)) = next else {
                return Ok(pushed);
            };
            client.push(&batch)?;
            pushed += batch.spans.len() as u64;
            if !self.inner.store().advance_sync_position(
                generation,
                store::meta::PUSHED_REV,
                max_rev,
            )? {
                return Ok(pushed);
            }
        }
    }

    /// Returns `(pulled spans, skipped batches)`. Stops early, without error,
    /// once the sync config changes under it.
    fn pull(&self, client: &sync::Client, generation: i64) -> Result<(u64, u64), SpentDayError> {
        let mut pulled = 0;
        let mut skipped = 0;
        let mut rescanned = false;
        loop {
            let (cursor, own_id) = {
                let store = self.inner.store();
                (
                    store.meta_i64(store::meta::PULL_CURSOR)?,
                    store.device_id.clone(),
                )
            };
            let page = client.pull(cursor)?;
            // The cursor is past the newest batch the server has. Cursors are
            // not authenticated, so a server or anything in between could have
            // sent a bogus one. Merging is idempotent by rev, so read the whole
            // space again, at most once per sync so a server that keeps lying
            // cannot keep this loop going.
            if cursor > page.head && !rescanned {
                rescanned = true;
                if !self.inner.store().reset_pull_cursor(generation)? {
                    return Ok((pulled, skipped));
                }
                continue;
            }
            for item in &page.batches {
                let batch = if item.envelope.device_id == own_id {
                    None
                } else {
                    let batch = client.keys().open(&item.envelope).ok();
                    skipped += u64::from(batch.is_none());
                    batch
                };
                let mut store = self.inner.store();
                let written = match &batch {
                    None => store.advance_sync_position(
                        generation,
                        store::meta::PULL_CURSOR,
                        item.cursor,
                    )?,
                    Some(batch) => store.merge_remote(
                        generation,
                        &batch.device.id,
                        &batch.device.name,
                        batch.device.platform,
                        &batch.spans,
                        item.cursor,
                    )?,
                };
                if !written {
                    return Ok((pulled, skipped));
                }
                pulled += batch.map_or(0, |b| b.spans.len() as u64);
            }
            if !page.more || page.batches.is_empty() {
                return Ok((pulled, skipped));
            }
        }
    }
}

#[uniffi::export]
impl SpentDay {
    /// Opens or creates the database. The device id is generated on first open and
    /// kept; `device_name` and `platform` are updated on every open.
    #[uniffi::constructor]
    pub fn open(
        db_path: String,
        device_name: String,
        platform: Platform,
    ) -> Result<std::sync::Arc<Self>, SpentDayError> {
        let store = store::Store::open(&db_path, true)?;
        let device_name = web::clip(device_name, web::MAX_DEVICE_NAME_BYTES);
        store.put_device(&store.device_id, &device_name, platform)?;
        Self::from_store(store)
    }

    pub fn device(&self) -> DeviceInfo {
        own_device(&self.inner.store())
    }

    /// Heartbeat path (macOS, Linux). `None` means the user is away.
    pub fn heartbeat(&self, at_ms: Millis, activity: Option<Activity>) -> Result<(), SpentDayError> {
        tracker::heartbeat(&mut self.inner.store(), at_ms, activity)
    }

    /// Heartbeat path, for an idle timeout. The user left at `last_input_ms`:
    /// behaves like `heartbeat(now_ms, None)`, then cuts every heartbeat span
    /// back to `last_input_ms`, including spans already closed after it.
    pub fn away_since(&self, last_input_ms: Millis, now_ms: Millis) -> Result<(), SpentDayError> {
        tracker::away_since(&mut self.inner.store(), last_input_ms, now_ms)
    }

    /// Interval path (Android UsageStats). Idempotent for the same `(app_id, start_ms)`.
    /// `end_ms <= start_ms` retracts the interval stored for `(app_id, start_ms)`:
    /// every piece becomes zero-length with a new `rev`, so other devices drop it
    /// too. With no such interval it does nothing.
    pub fn record_interval(
        &self,
        start_ms: Millis,
        end_ms: Millis,
        activity: Activity,
    ) -> Result<(), SpentDayError> {
        tracker::record_interval(&mut self.inner.store(), start_ms, end_ms, activity)
    }

    /// Active-tab report from the browser extension or the Android accessibility service.
    pub fn web_hint(
        &self,
        at_ms: Millis,
        browser: String,
        page: WebPage,
    ) -> Result<(), SpentDayError> {
        self.inner.web_hint(at_ms, &browser, Some(&page))
    }

    /// The browser shows no web page now (new tab, `chrome://`, a PDF, or a
    /// private window), so time from `at_ms` on goes to no site until the
    /// next `web_hint`.
    pub fn web_cleared(&self, at_ms: Millis, browser: String) -> Result<(), SpentDayError> {
        self.inner.web_hint(at_ms, &browser, None)
    }

    /// Closes the open span. Call on sleep, lock and quit.
    pub fn flush(&self, at_ms: Millis) -> Result<(), SpentDayError> {
        tracker::close(&mut self.inner.store(), at_ms)
    }

    pub fn summary(&self, query: SummaryQuery) -> Result<Summary, SpentDayError> {
        let store = self.inner.store();
        let spans =
            store.spans_between(query.start_ms, query.end_ms, query.device_id.as_deref())?;
        summary::summarize(&query, &spans, &store.devices()?)
    }

    pub fn day(&self, query: DayQuery) -> Result<DayDetail, SpentDayError> {
        self.inner.store().snapshot(|store| {
            let spans = store.spans_between(query.start_ms, query.end_ms, None)?;
            Ok(day::detail(&query, &spans, &store.devices()?))
        })
    }

    /// Every device known locally, this one first.
    pub fn devices(&self) -> Result<Vec<DeviceInfo>, SpentDayError> {
        self.inner.store().devices()
    }

    /// A fresh random sync code for a new account.
    pub fn new_sync_code(&self) -> String {
        sync::new_code()
    }

    pub fn sync_config(&self) -> Result<Option<SyncConfig>, SpentDayError> {
        let (config, _) = self.inner.store().sync_config()?;
        Ok(config.map(|(server_url, sync_code)| SyncConfig {
            server_url,
            sync_code,
        }))
    }

    /// Validates the code and stores the config. `None` turns sync off. A sync
    /// already running for the old config stops without writing anything more.
    pub fn set_sync_config(&self, config: Option<SyncConfig>) -> Result<(), SpentDayError> {
        let config = match config {
            Some(c) => {
                let server_url = sync::check_server_url(c.server_url.trim())?;
                Some((server_url, sync::canonical_code(&c.sync_code)?))
            }
            None => None,
        };
        let mut store = self.inner.store();
        if store.sync_config()?.0 == config {
            return Ok(());
        }
        store.set_sync_config(config.as_ref().map(|(s, c)| (s.as_str(), c.as_str())))
    }

    /// A call made while another is running waits for it, then runs.
    pub fn sync_now(&self) -> Result<SyncReport, SpentDayError> {
        let _running = self.syncing.lock().unwrap_or_else(|p| p.into_inner());
        let (client, generation) = self.sync_client()?;
        // A push that keeps failing must not also stop this device from
        // receiving everyone else's data.
        let pushed = self.push(&client, generation);
        let pulled = self.pull(&client, generation);
        let pushed_spans = pushed?;
        let (pulled_spans, skipped_batches) = pulled?;
        Ok(SyncReport {
            pushed_spans,
            pulled_spans,
            skipped_batches,
            finished_at_ms: now_ms(),
        })
    }

    /// Serves `POST /v1/web` and `/v1/health` on 127.0.0.1:`port` for the browser extension, on a
    /// background thread. Only requests with a local `Host` and an extension `Origin` are served.
    /// Desktop only. Calling it twice is a no-op.
    pub fn start_web_listener(&self, port: u16) -> Result<(), SpentDayError> {
        let mut started = self
            .listener_started
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !*started {
            listener::spawn(self.inner.clone(), port)?;
            *started = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sync that read its config, then paused while the user switched
    /// accounts and kept working. Its server is unreachable, so any upload
    /// attempt with the old credentials shows up as an error.
    #[test]
    fn a_paused_sync_never_pushes_after_an_account_switch() {
        let dir = tempfile::tempdir().unwrap();
        let m = SpentDay::open(
            dir.path().join("m.db").to_string_lossy().into_owned(),
            "box".into(),
            Platform::Linux,
        )
        .unwrap();
        let config = |sync_code| {
            Some(SyncConfig {
                server_url: "http://127.0.0.1:1".into(),
                sync_code,
            })
        };
        m.set_sync_config(config(m.new_sync_code())).unwrap();
        let (old_client, old_generation) = m.sync_client().unwrap();

        m.set_sync_config(config(m.new_sync_code())).unwrap();
        let activity = Activity {
            app_id: "slack".into(),
            app_name: "Slack".into(),
            web: None,
        };
        m.record_interval(0, 10_000, activity).unwrap();
        assert_eq!(m.push(&old_client, old_generation).unwrap(), 0);
    }

    /// A config stored before the https rule, written straight into the
    /// database as an older build would have.
    fn seeded(server_url: &str) -> (tempfile::TempDir, Arc<SpentDay>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.db").to_string_lossy().into_owned();
        let m = SpentDay::open(path, "box".into(), Platform::Linux).unwrap();
        let code = m.new_sync_code();
        {
            let store = m.inner.store();
            store.set_meta(store::meta::SYNC_SERVER, Some(server_url)).unwrap();
            store.set_meta(store::meta::SYNC_CODE, Some(&code)).unwrap();
        }
        (dir, m)
    }

    #[test]
    fn a_stored_server_url_is_checked_every_time_sync_runs() {
        for refused in ["http://nas.local:8787", "http://sync.example.com", "ftp://x"] {
            let (_dir, m) = seeded(refused);
            assert!(
                matches!(m.sync_client(), Err(SpentDayError::InvalidArgument(_))),
                "{refused}"
            );
            assert!(matches!(m.sync_now(), Err(SpentDayError::InvalidArgument(_))));
        }
        for (stored, dialed) in [
            ("http://localhost:8787", "http://127.0.0.1:8787"),
            ("http://LOCALHOST.:8787/", "http://127.0.0.1:8787"),
            ("http://10.0.0.1.:8787", "http://10.0.0.1:8787"),
        ] {
            let (_dir, m) = seeded(stored);
            let (client, _) = m.sync_client().unwrap();
            assert_eq!(client.base(), dialed);
        }
    }
}
