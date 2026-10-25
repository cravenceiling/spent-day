//! SQLite storage. One connection per `SpentDay`, guarded by its mutex. Other
//! handles may have the same file open.

use std::collections::HashSet;

use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{DeviceInfo, SpentDayError, Millis, Platform};

/// One row of the `spans` table. Rust-only: the CLI and the sync wire format use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub device_id: String,
    pub id: String,
    pub start_ms: Millis,
    pub end_ms: Millis,
    pub app_id: String,
    pub app_name: String,
    pub domain: Option<String>,
    pub rev: i64,
}

/// An empty `domain` is a no-page hint (see `web::record_hint`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub at_ms: Millis,
    pub domain: String,
}

/// The schema stores apps, domains and times. It has no column for a URL or a
/// title, so neither can reach the file or a sync batch.
const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
    CREATE TABLE devices (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        platform TEXT NOT NULL
    ) STRICT;
    CREATE TABLE spans (
        device_id TEXT NOT NULL,
        id TEXT NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER NOT NULL,
        app_id TEXT NOT NULL,
        app_name TEXT NOT NULL,
        domain TEXT,
        rev INTEGER NOT NULL,
        -- Local only, never synced: which interval (see tracker::record_interval)
        -- wrote this span, so a re-ingest can find pieces its new split dropped.
        interval_id TEXT,
        PRIMARY KEY (device_id, id)
    ) STRICT;
    CREATE INDEX spans_start ON spans (start_ms);
    CREATE INDEX spans_device_end ON spans (device_id, end_ms);
    CREATE INDEX spans_device_rev ON spans (device_id, rev);
    CREATE TABLE web_hints (
        at_ms INTEGER NOT NULL,
        browser TEXT NOT NULL,
        known_browser INTEGER NOT NULL,
        domain TEXT NOT NULL
    ) STRICT;
    CREATE INDEX web_hints_browser_at ON web_hints (browser, at_ms);
    CREATE INDEX web_hints_at ON web_hints (at_ms);
    CREATE INDEX spans_interval ON spans (interval_id) WHERE interval_id IS NOT NULL;
"#];

pub mod meta {
    pub const DEVICE_ID: &str = "device_id";
    pub const OPEN_SPAN: &str = "open_span";
    pub const SYNC_SERVER: &str = "sync_server_url";
    pub const SYNC_CODE: &str = "sync_code";
    pub const PUSHED_REV: &str = "pushed_rev";
    pub const PULL_CURSOR: &str = "pull_cursor";
    /// Bumped whenever the sync config changes. A sync started under another
    /// generation must not move this one's positions.
    pub const SYNC_GENERATION: &str = "sync_generation";
}

const SPAN_COLUMNS: &str = "device_id, id, start_ms, end_ms, app_id, app_name, domain, rev";

pub struct Store {
    conn: Connection,
    pub device_id: String,
}

fn platform_str(p: Platform) -> &'static str {
    match p {
        Platform::Macos => "macos",
        Platform::Linux => "linux",
        Platform::Android => "android",
        Platform::Ios => "ios",
        Platform::Windows => "windows",
    }
}

/// Unknown names (a platform added by a newer build) read as Linux.
pub fn parse_platform(s: &str) -> Platform {
    match s {
        "macos" => Platform::Macos,
        "android" => Platform::Android,
        "ios" => Platform::Ios,
        "windows" => Platform::Windows,
        _ => Platform::Linux,
    }
}

fn span_from_row(row: &Row) -> rusqlite::Result<Span> {
    Ok(Span {
        device_id: row.get(0)?,
        id: row.get(1)?,
        start_ms: row.get(2)?,
        end_ms: row.get(3)?,
        app_id: row.get(4)?,
        app_name: row.get(5)?,
        domain: row.get(6)?,
        rev: row.get(7)?,
    })
}

fn hint_from_row(row: &Row) -> rusqlite::Result<Hint> {
    Ok(Hint {
        at_ms: row.get(0)?,
        domain: row.get(1)?,
    })
}

impl From<rusqlite::Error> for SpentDayError {
    fn from(e: rusqlite::Error) -> Self {
        SpentDayError::Storage(e.to_string())
    }
}

type Result<T> = std::result::Result<T, SpentDayError>;

impl Store {
    /// Opens the database, creating it when `create` is set. Fails when the
    /// database has no device yet and `create` is not set.
    pub fn open(path: &str, create: bool) -> Result<Store> {
        // NOFOLLOW: SQLite refuses a path with a symlink in it, so one swapped
        // in after `make_private` checked the path is not opened. The path it
        // gets has its folders resolved, so a symlinked folder such as macOS's
        // /tmp still works.
        let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        if create {
            flags |= OpenFlags::SQLITE_OPEN_CREATE;
        }
        let real = make_private(path, create).map_err(|e| match unreadable(&e) {
            // It already names the file, in the command that fixes it.
            Some(unreadable) => SpentDayError::Storage(unreadable.to_string()),
            None => SpentDayError::Storage(format!("cannot make {path} private: {e}")),
        })?;
        let mut conn = Connection::open_with_flags(&real, flags)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrate(&mut conn, path)?;

        let existing: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                [meta::DEVICE_ID],
                |r| r.get(0),
            )
            .optional()?;
        let device_id = match existing {
            Some(id) => id,
            None if create => {
                let id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                    params![meta::DEVICE_ID, id],
                )?;
                id
            }
            None => {
                return Err(SpentDayError::Storage(format!(
                    "{path} has no Spent Day device yet"
                )));
            }
        };
        Ok(Store { conn, device_id })
    }

    /// Runs `read` in one read transaction, so all it reads comes from one
    /// snapshot even while other handles commit.
    pub fn snapshot<T>(&self, read: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
        let _tx = self.conn.unchecked_transaction()?;
        read(self)
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn meta_i64(&self, key: &str) -> Result<i64> {
        Ok(self.meta(key)?.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    pub fn set_meta(&self, key: &str, value: Option<&str>) -> Result<()> {
        put_meta(&self.conn, key, value)
    }

    pub fn put_device(&self, id: &str, name: &str, platform: Platform) -> Result<()> {
        self.conn.execute(
            "INSERT INTO devices (id, name, platform) VALUES (?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, platform = excluded.platform",
            params![id, name, platform_str(platform)],
        )?;
        Ok(())
    }

    /// Every device, this one first, then by last activity.
    pub fn devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.id, d.name, d.platform,
                    COALESCE((SELECT MAX(end_ms) FROM spans s WHERE s.device_id = d.id), 0) AS last_seen
             FROM devices d
             ORDER BY d.id = ?1 DESC, last_seen DESC, d.name",
        )?;
        let rows = stmt.query_map([&self.device_id], |r| {
            Ok(DeviceInfo {
                id: r.get(0)?,
                name: r.get(1)?,
                platform: parse_platform(&r.get::<_, String>(2)?),
                last_seen_ms: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn span(&self, id: &str) -> Result<Option<Span>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SPAN_COLUMNS} FROM spans WHERE device_id = ?1 AND id = ?2"),
                params![self.device_id, id],
                span_from_row,
            )
            .optional()?)
    }

    /// Writes one of this device's spans. Bumps `rev` only when the row changes,
    /// so re-recording an identical span is a no-op for sync.
    pub fn put_own_span(&mut self, span: Span) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        upsert_own(&tx, &self.device_id, span, None)?;
        tx.commit()?;
        Ok(())
    }

    /// Writes the pieces of one interval. Pieces an earlier write of the same
    /// interval had but this split lacks become zero-length, with a new rev, so
    /// other devices drop their time too. No pieces retracts the interval.
    pub fn put_interval(&mut self, interval_id: &str, pieces: Vec<Span>) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut stale: Vec<Span> = {
            let mut stmt = tx.prepare(&format!(
                "SELECT {SPAN_COLUMNS} FROM spans
                 WHERE device_id = ?1 AND interval_id = ?2 AND end_ms > start_ms"
            ))?;
            stmt.query_map(params![self.device_id, interval_id], span_from_row)?
                .collect::<rusqlite::Result<_>>()?
        };
        let kept: HashSet<&str> = pieces.iter().map(|p| p.id.as_str()).collect();
        stale.retain(|old| !kept.contains(old.id.as_str()));
        for piece in pieces {
            upsert_own(&tx, &self.device_id, piece, Some(interval_id))?;
        }
        for mut old in stale {
            old.end_ms = old.start_ms;
            upsert_own(&tx, &self.device_id, old, Some(interval_id))?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Writes `open`, forgets it as the open span, and cuts this device's
    /// heartbeat spans back to `cut_ms`, in one transaction. A span crossing
    /// the cut ends there. A span after it becomes zero-length at the cut, with
    /// a new rev so other devices drop its time too, and without holding
    /// `own_spans_end` past the cut.
    pub fn close_heartbeat_spans(&mut self, open: Option<Span>, cut_ms: Millis) -> Result<()> {
        if open.is_none() && cut_ms == Millis::MAX {
            return Ok(());
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(open) = open {
            upsert_own(&tx, &self.device_id, open, None)?;
        }
        put_meta(&tx, meta::OPEN_SPAN, None)?;
        let late: Vec<Span> = {
            let mut stmt = tx.prepare(&format!(
                "SELECT {SPAN_COLUMNS} FROM spans
                 WHERE device_id = ?1 AND end_ms > ?2 AND interval_id IS NULL"
            ))?;
            stmt.query_map(params![self.device_id, cut_ms], span_from_row)?
                .collect::<rusqlite::Result<_>>()?
        };
        for mut span in late {
            span.start_ms = span.start_ms.min(cut_ms);
            span.end_ms = cut_ms;
            upsert_own(&tx, &self.device_id, span, None)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The sync config and its generation, from one snapshot.
    pub fn sync_config(&self) -> Result<(Option<(String, String)>, i64)> {
        self.snapshot(|store| {
            let server = store.meta(meta::SYNC_SERVER)?;
            let code = store.meta(meta::SYNC_CODE)?;
            Ok((server.zip(code), store.meta_i64(meta::SYNC_GENERATION)?))
        })
    }

    /// Stores a new sync config, resets both sync positions and bumps the
    /// generation, all in one transaction.
    pub fn set_sync_config(&mut self, config: Option<(&str, &str)>) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (key, value) in [
            (meta::SYNC_SERVER, config.map(|c| c.0)),
            (meta::SYNC_CODE, config.map(|c| c.1)),
            (meta::PUSHED_REV, None),
            (meta::PULL_CURSOR, None),
        ] {
            put_meta(&tx, key, value)?;
        }
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, '1')
             ON CONFLICT (key) DO UPDATE SET value = CAST(value AS INTEGER) + 1",
            [meta::SYNC_GENERATION],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Moves `key` (`PUSHED_REV` or `PULL_CURSOR`) forward to `value`, unless
    /// the sync config changed since `generation`. Returns whether it wrote.
    pub fn advance_sync_position(
        &mut self,
        generation: i64,
        key: &str,
        value: i64,
    ) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !same_generation(&tx, generation)? {
            return Ok(false);
        }
        advance(&tx, key, value)?;
        tx.commit()?;
        Ok(true)
    }

    /// Sets the pull cursor back to `0`, so the next pull reads the whole
    /// space again, unless the sync config changed since `generation`.
    /// Returns whether it wrote.
    pub fn reset_pull_cursor(&mut self, generation: i64) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !same_generation(&tx, generation)? {
            return Ok(false);
        }
        put_meta(&tx, meta::PULL_CURSOR, None)?;
        tx.commit()?;
        Ok(true)
    }

    /// Applies a batch of spans from another device (the highest `rev` wins)
    /// and moves the pull cursor past it, unless the sync config changed since
    /// `generation`. Returns whether it wrote.
    pub fn merge_remote(
        &mut self,
        generation: i64,
        device_id: &str,
        name: &str,
        platform: Platform,
        spans: &[Span],
        cursor: i64,
    ) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !same_generation(&tx, generation)? {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO devices (id, name, platform) VALUES (?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, platform = excluded.platform",
            params![device_id, name, platform_str(platform)],
        )?;
        {
            let mut stmt = tx.prepare(&format!(
                "INSERT INTO spans ({SPAN_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (device_id, id) DO UPDATE SET
                    start_ms = excluded.start_ms, end_ms = excluded.end_ms, app_id = excluded.app_id,
                    app_name = excluded.app_name, domain = excluded.domain, rev = excluded.rev
                 WHERE excluded.rev > spans.rev"
            ))?;
            for s in spans {
                stmt.execute(params![
                    device_id, s.id, s.start_ms, s.end_ms, s.app_id, s.app_name, s.domain, s.rev
                ])?;
            }
        }
        advance(&tx, meta::PULL_CURSOR, cursor)?;
        tx.commit()?;
        Ok(true)
    }

    /// The latest `end_ms` among this device's spans. Its span table is the
    /// persisted high-water mark, so a cut (`away_since`) lowers it again.
    pub fn own_spans_end(&self) -> Result<Option<Millis>> {
        Ok(self.conn.query_row(
            "SELECT MAX(end_ms) FROM spans WHERE device_id = ?1",
            [&self.device_id],
            |r| r.get(0),
        )?)
    }

    pub fn own_spans_after_rev(&self, rev: i64, limit: usize) -> Result<Vec<Span>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SPAN_COLUMNS} FROM spans WHERE device_id = ?1 AND rev > ?2 ORDER BY rev LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![self.device_id, rev, limit as i64], span_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Spans overlapping `[start_ms, end_ms)`.
    pub fn spans_between(
        &self,
        start_ms: Millis,
        end_ms: Millis,
        device_id: Option<&str>,
    ) -> Result<Vec<Span>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SPAN_COLUMNS} FROM spans
             WHERE start_ms < ?2 AND end_ms > ?1 AND (?3 IS NULL OR device_id = ?3)"
        ))?;
        let rows = stmt.query_map(params![start_ms, end_ms, device_id], span_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn recent_spans(&self, limit: usize) -> Result<Vec<Span>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SPAN_COLUMNS} FROM spans ORDER BY start_ms DESC, end_ms DESC LIMIT ?1"
        ))?;
        let rows = stmt.query_map([limit as i64], span_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn add_hint(
        &self,
        at_ms: Millis,
        browser: &str,
        known: bool,
        domain: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO web_hints (at_ms, browser, known_browser, domain) VALUES (?1, ?2, ?3, ?4)",
            params![at_ms, browser, known, domain],
        )?;
        Ok(())
    }

    /// Latest hint in `[from_ms, at_ms]` naming `browser`, or, with
    /// `unknown_too`, a browser the app table does not know.
    pub fn latest_hint(
        &self,
        browser: &str,
        unknown_too: bool,
        from_ms: Millis,
        at_ms: Millis,
    ) -> Result<Option<Hint>> {
        Ok(self
            .conn
            .query_row(
                "SELECT at_ms, domain FROM web_hints
                 WHERE (browser = ?1 OR (?2 AND known_browser = 0)) AND at_ms >= ?3 AND at_ms <= ?4
                 ORDER BY at_ms DESC, rowid DESC LIMIT 1",
                params![browser, unknown_too, from_ms, at_ms],
                hint_from_row,
            )
            .optional()?)
    }

    /// Hints naming `browser` strictly inside `(start_ms, end_ms)`, oldest first.
    pub fn hints_inside(
        &self,
        browser: &str,
        start_ms: Millis,
        end_ms: Millis,
    ) -> Result<Vec<Hint>> {
        let mut stmt = self.conn.prepare(
            "SELECT at_ms, domain FROM web_hints WHERE browser = ?1 AND at_ms > ?2 AND at_ms < ?3
             ORDER BY at_ms, rowid",
        )?;
        let rows = stmt.query_map(params![browser, start_ms, end_ms], hint_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Drops hints more than `keep_ms` older than the newest one, or than
    /// `now_ms` when that is earlier.
    pub fn prune_hints(&self, keep_ms: Millis, now_ms: Millis) -> Result<()> {
        self.conn.execute(
            "DELETE FROM web_hints WHERE at_ms < MIN((SELECT MAX(at_ms) FROM web_hints), ?2) - ?1",
            [keep_ms, now_ms],
        )?;
        Ok(())
    }
}

/// The database holds the sync code in plain text, so on Unix only its owner
/// may read it, whatever the umask. With `create`, the directories this call
/// creates are made `0700`, and a missing file is created `0600` before SQLite
/// opens it. Existing directories are left alone. SQLite gives the `-wal` and
/// `-shm` files it creates later the mode of the database file, and existing
/// ones are set to `0600`. A database path that is a symlink or a `file:` URI is
/// refused, because the sidecars would not sit where they are protected. Only
/// a true in-memory database (`:memory:` or an empty path) is exempt. Any
/// protection that fails is an error.
///
/// Returns the path to hand SQLite: on Unix, `path` with its folders resolved.
/// It stays a `PathBuf`, because a resolved folder may have a name that is not
/// UTF-8, and a lossy conversion would point SQLite at another file.
fn make_private(path: &str, create: bool) -> std::io::Result<std::path::PathBuf> {
    if path.is_empty() || path == ":memory:" {
        return Ok(path.into());
    }
    if path.to_ascii_lowercase().starts_with("file:") {
        return Err(std::io::Error::other("file: URIs are not supported, pass a plain path"));
    }
    #[cfg(unix)]
    {
        let file = std::path::Path::new(path);
        protect(file, create)?;
        let dir = file.parent().filter(|d| !d.as_os_str().is_empty());
        let (Some(name), Ok(dir)) = (
            file.file_name(),
            std::fs::canonicalize(dir.unwrap_or(std::path::Path::new("."))),
        ) else {
            // No such folder: SQLite fails to open it and says so.
            return Ok(path.into());
        };
        Ok(dir.join(name))
    }
    #[cfg(not(unix))]
    {
        let _ = create;
        Ok(path.into())
    }
}

/// A database file or sidecar the core can't read. Its message is the shell
/// command that fixes it, with the path quoted, since the macOS default has a
/// space in it.
#[derive(Debug)]
struct Unreadable(std::path::PathBuf);

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quoted = self.0.to_string_lossy().replace('\'', "'\\''");
        write!(f, "Spent Day can't read one of its database files. Run chmod 600 '{quoted}', then try again.")
    }
}

impl std::error::Error for Unreadable {}

fn unreadable(e: &std::io::Error) -> Option<&Unreadable> {
    e.get_ref()?.downcast_ref()
}

#[cfg(unix)]
fn protect(file: &std::path::Path, create: bool) -> std::io::Result<()> {
    use std::fs::{DirBuilder, OpenOptions, Permissions};
    use std::io::ErrorKind;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::PathBuf;

    let symlink = || std::io::Error::other("it is a symlink, pass the real path");
    // Opens `path` only if it is the regular file `lstat` saw: a symlink is
    // refused, and one swapped in between the check and the open shows up as a
    // different inode.
    let open_file = |path: &std::path::Path| -> std::io::Result<Option<std::fs::File>> {
        let seen = match std::fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if seen.file_type().is_symlink() {
            return Err(symlink());
        }
        if !seen.is_file() {
            return Err(std::io::Error::other(format!("{} is not a file", path.display())));
        }
        // A file of ours with no read bit, such as 0200, is not changed by
        // path: chmod follows a symlink swapped in after the check above.
        let opened = OpenOptions::new().read(true).open(path).map_err(|e| {
            if e.kind() == ErrorKind::PermissionDenied {
                std::io::Error::new(e.kind(), Unreadable(path.to_owned()))
            } else {
                e
            }
        })?;
        let meta = opened.metadata()?;
        if (meta.dev(), meta.ino()) != (seen.dev(), seen.ino()) {
            return Err(symlink());
        }
        Ok(Some(opened))
    };
    if create {
        if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
            // One folder at a time, so only the folders this call creates are
            // made private, whatever `..` or `.` the path holds.
            let mut sofar = PathBuf::new();
            for part in dir.components() {
                sofar.push(part);
                match DirBuilder::new().mode(0o700).create(&sofar) {
                    // Through a handle, not by path: O_NOFOLLOW refuses a
                    // symlink renamed in after the mkdir, so no other folder
                    // is changed.
                    Ok(()) => OpenOptions::new()
                        .read(true)
                        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                        .open(&sofar)?
                        .set_permissions(Permissions::from_mode(0o700))?,
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
        }
        // O_EXCL creates the file itself, and never through a symlink.
        match OpenOptions::new().write(true).create_new(true).mode(0o600).open(file) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    // The database and each sidecar SQLite may have made: never a symlink, and
    // changed through the open file, so the check and the change are one file.
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = file.as_os_str().to_owned();
        name.push(suffix);
        if let Some(opened) = open_file(std::path::Path::new(&name))? {
            opened.set_permissions(Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

fn put_meta(conn: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(v) => conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, v],
        )?,
        None => conn.execute("DELETE FROM meta WHERE key = ?1", [key])?,
    };
    Ok(())
}

fn same_generation(conn: &Connection, generation: i64) -> Result<bool> {
    let current: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [meta::SYNC_GENERATION],
            |r| r.get(0),
        )
        .optional()?;
    Ok(current.and_then(|v| v.parse().ok()).unwrap_or(0) == generation)
}

/// Sync positions only move forward, so two syncs racing on one database
/// cannot move one back.
fn advance(conn: &Connection, key: &str, value: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE
         SET value = MAX(CAST(value AS INTEGER), CAST(excluded.value AS INTEGER))",
        params![key, value.to_string()],
    )?;
    Ok(())
}

/// The rev comes from the database inside the caller's write transaction, so
/// two handles on one file (an app and the CLI, or two Android components)
/// never hand out the same rev.
fn upsert_own(
    tx: &rusqlite::Transaction,
    device_id: &str,
    span: Span,
    interval_id: Option<&str>,
) -> Result<()> {
    let rev: i64 = tx.query_row(
        "SELECT COALESCE(MAX(rev), 0) + 1 FROM spans WHERE device_id = ?1",
        [device_id],
        |r| r.get(0),
    )?;
    tx.execute(
        &format!(
            "INSERT INTO spans ({SPAN_COLUMNS}, interval_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (device_id, id) DO UPDATE SET
                start_ms = excluded.start_ms, end_ms = excluded.end_ms, app_id = excluded.app_id,
                app_name = excluded.app_name, domain = excluded.domain, rev = excluded.rev,
                interval_id = excluded.interval_id
             WHERE start_ms IS NOT excluded.start_ms OR end_ms IS NOT excluded.end_ms
                OR app_id IS NOT excluded.app_id OR app_name IS NOT excluded.app_name
                OR domain IS NOT excluded.domain OR interval_id IS NOT excluded.interval_id"
        ),
        params![
            device_id,
            span.id,
            span.start_ms,
            span.end_ms,
            span.app_id,
            span.app_name,
            span.domain,
            rev,
            interval_id
        ],
    )?;
    Ok(())
}

fn migrate(conn: &mut Connection, path: &str) -> Result<()> {
    // Development builds stored URLs and titles. There is no migration from
    // them, so say so instead of failing on a column that is not there.
    let early: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('spans') WHERE name IN ('url', 'title')",
        [],
        |r| r.get(0),
    )?;
    if early > 0 {
        return Err(SpentDayError::OldDatabase { path: path.to_owned() });
    }
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let version = version as usize;
    if version > MIGRATIONS.len() {
        return Err(SpentDayError::Storage(format!(
            "database schema v{version} is newer than this build (v{})",
            MIGRATIONS.len()
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}
