//! What the core writes to disk: domains and app names, never a URL's path,
//! query or fragment, never a title, and only into files its owner can read.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use spentday_core::{Activity, Platform, SpentDay, WebPage};

/// Every secret the inputs below carry. None may reach the database.
const SECRETS: &[&str] = &["secret-path", "secret-query", "secret-fragment", "Secret Title"];
const URL: &str = "https://www.example.com/secret-path/page?q=secret-query#secret-fragment";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The database file and its sidecars, as bytes.
fn on_disk(dir: &tempfile::TempDir) -> Vec<u8> {
    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        bytes.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    bytes
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn urls_and_titles_never_reach_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.db").to_string_lossy().into_owned();
    let m = SpentDay::open(path.clone(), "box".into(), Platform::Linux).unwrap();
    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    };
    let now = now_ms();
    let page = || WebPage { url: URL.into() };
    let chrome = Activity {
        app_id: "com.google.Chrome".into(),
        app_name: "Google Chrome".into(),
        web: None,
    };

    // A collector that read the URL itself (macOS AppleScript).
    let read = Activity {
        web: Some(page()),
        ..chrome.clone()
    };
    m.heartbeat(now - 60_000, Some(read.clone())).unwrap();
    m.heartbeat(now - 55_000, Some(read.clone())).unwrap();
    m.record_interval(now - 50_000, now - 45_000, read).unwrap();
    // A hint from the Android accessibility service, on both paths.
    m.web_hint(now - 40_000, "com.android.chrome".into(), page())
        .unwrap();
    m.record_interval(
        now - 40_000,
        now - 35_000,
        Activity {
            app_id: "com.android.chrome".into(),
            ..chrome.clone()
        },
    )
    .unwrap();

    // The extension, sending more than it should: a full URL and a title.
    let port = free_port();
    m.start_web_listener(port).unwrap();
    let body = format!(r#"{{"browser":"chrome","url":"{URL}","title":"Secret Title"}}"#);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "POST /v1/web HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: chrome-extension://x\r\n\
         Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 204 "), "{answer}");
    // The listener stamps the hint when it arrives.
    let later = now_ms();
    m.heartbeat(later, Some(chrome.clone())).unwrap();
    m.heartbeat(later + 5_000, Some(chrome)).unwrap();
    m.flush(later + 5_000).unwrap();

    let spans = m.recent_spans(10).unwrap();
    assert_eq!(spans.len(), 4);
    assert!(
        spans
            .iter()
            .all(|s| s.domain.as_deref() == Some("example.com")),
        "{spans:?}"
    );
    let rows = serde_json::to_string(&spans).unwrap();
    for secret in SECRETS {
        assert!(!rows.contains(secret), "{secret} in {rows}");
    }
    // While the WAL still holds the writes, and after closing moved them
    // into the database file.
    let open = on_disk(&dir);
    drop(m);
    for bytes in [open, on_disk(&dir)] {
        assert!(contains(&bytes, "example.com"));
        for secret in SECRETS {
            assert!(!contains(&bytes, secret), "{secret} is on disk");
        }
    }
}

#[cfg(unix)]
#[test]
fn the_database_and_its_directory_are_private_whatever_the_umask() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let set = |path: &std::path::Path, mode: u32| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap()
    };
    let write = |m: &SpentDay| {
        m.heartbeat(
            1_000,
            Some(Activity {
                app_id: "slack".into(),
                app_name: "Slack".into(),
                web: None,
            }),
        )
        .unwrap();
    };

    // A fresh install in a directory that does not exist yet.
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("data").join("spentday");
    let db = dir.join("spentday.db");
    let m = SpentDay::open(db.to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap();
    write(&m);
    assert_eq!(mode(&dir), 0o700);
    for file in ["spentday.db", "spentday.db-wal", "spentday.db-shm"] {
        assert_eq!(mode(&dir.join(file)), 0o600, "{file}");
    }
    drop(m);

    // Files from an install made under umask 022 are tightened. The folder
    // already exists, so it is the user's to choose and stays as it is.
    set(&dir, 0o755);
    for file in ["spentday.db", "spentday.db-wal", "spentday.db-shm"] {
        if dir.join(file).exists() {
            set(&dir.join(file), 0o644);
        }
    }
    let m = SpentDay::open(db.to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap();
    write(&m);
    assert_eq!(mode(&dir), 0o755);
    for file in ["spentday.db", "spentday.db-wal", "spentday.db-shm"] {
        assert_eq!(mode(&dir.join(file)), 0o600, "{file}");
    }
    drop(m);

    // Tools that only read another app's database tighten its files too.
    set(&db, 0o644);
    drop(SpentDay::open_existing(&db.to_string_lossy()).unwrap());
    assert_eq!(mode(&db), 0o600);
}

/// A database in a directory another user owns, such as `/tmp`, opens and
/// gets private files, and the directory is left alone.
#[cfg(unix)]
#[test]
fn a_database_in_a_shared_directory_leaves_the_directory_alone() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = std::path::Path::new("/tmp");
    if !tmp.is_dir() {
        return;
    }
    let before = std::fs::metadata(tmp).unwrap().permissions().mode();
    let db = tmp.join(format!("spentday-privacy-{}.db", std::process::id()));
    let m = SpentDay::open(db.to_string_lossy().into_owned(), "box".into(), Platform::Linux);
    let mode = std::fs::metadata(&db).map(|m| m.permissions().mode() & 0o777);
    drop(m);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
    }
    assert_eq!(mode.unwrap(), 0o600);
    assert_eq!(std::fs::metadata(tmp).unwrap().permissions().mode(), before);
}

/// Early builds stored URLs and titles. Their databases are refused with an
/// error of their own, so apps can tell it from a full disk, and nothing in
/// them is read.
#[test]
fn a_database_from_an_early_build_is_refused_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE spans (device_id TEXT, id TEXT, title TEXT, url TEXT);
         PRAGMA user_version = 2;",
    )
    .unwrap();
    drop(conn);
    let path = path.to_string_lossy().into_owned();
    let Err(e) = SpentDay::open(path.clone(), "box".into(), Platform::Linux) else {
        panic!("an early database opened");
    };
    assert!(matches!(&e, spentday_core::SpentDayError::OldDatabase { path: refused } if *refused == path), "{e:?}");
    assert!(e.to_string().contains("early test build"), "{e}");
}

/// Only the folders `open` creates are made private. A folder the user
/// already had, such as `~/projects`, keeps its mode, setgid bit included.
#[cfg(unix)]
#[test]
fn only_folders_the_core_creates_become_private() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    let root = tempfile::tempdir().unwrap();
    let projects = root.path().join("projects");
    std::fs::create_dir(&projects).unwrap();
    std::fs::set_permissions(&projects, std::fs::Permissions::from_mode(0o2775)).unwrap();
    let before = mode(&projects);

    let beside = projects.join("x.db");
    drop(SpentDay::open(beside.to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap());
    assert_eq!(mode(&projects), before);
    assert_eq!(mode(&beside) & 0o777, 0o600);

    let nested = projects.join("a").join("b").join("x.db");
    drop(SpentDay::open(nested.to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap());
    assert_eq!(mode(&projects), before);
    assert_eq!(mode(&projects.join("a")) & 0o777, 0o700);
    assert_eq!(mode(&projects.join("a").join("b")) & 0o777, 0o700);
}

/// SQLite follows a symlink to the real file and puts the WAL beside it,
/// out of reach of the repair, so a symlinked database is refused.
#[cfg(unix)]
#[test]
fn a_symlinked_database_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.db");
    drop(SpentDay::open(real.to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap());
    let link = dir.path().join("link.db");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let link = link.to_string_lossy().into_owned();
    for opened in [
        SpentDay::open(link.clone(), "box".into(), Platform::Linux).err(),
        SpentDay::open_existing(&link).err(),
    ] {
        match opened {
            Some(spentday_core::SpentDayError::Storage(why)) => assert!(why.contains("symlink"), "{why}"),
            other => panic!("expected a Storage error, got {other:?}"),
        }
    }
}

/// A `file:` URI would skip the permission repair, so it is refused. A plain
/// in-memory database still opens.
#[test]
fn a_file_uri_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let uri = format!("file:{}", dir.path().join("x.db").display());
    for uri in [uri.clone(), uri.replace("file:", "FILE:"), "file::memory:".to_owned()] {
        match SpentDay::open(uri.clone(), "box".into(), Platform::Linux) {
            Err(spentday_core::SpentDayError::Storage(why)) => assert!(why.contains("file: URIs"), "{why}"),
            other => panic!("{uri}: expected a Storage error, got {:?}", other.err()),
        }
    }
    assert!(!dir.path().join("x.db").exists());
    assert!(SpentDay::open(":memory:".into(), "box".into(), Platform::Linux).is_ok());
}

/// `..` in the path must not fool the created-folder rule: `existing/new/..`
/// is `existing`, which this call did not create.
#[cfg(unix)]
#[test]
fn dot_dot_in_the_path_leaves_the_existing_folder_alone() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o775)).unwrap();
    let db = existing.join("new").join("..").join("x.db");
    drop(SpentDay::open(db.to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap());
    assert_eq!(mode(&existing), 0o775);
    assert_eq!(mode(&existing.join("new")), 0o700);
    assert_eq!(mode(&existing.join("x.db")) & 0o777, 0o600);
}

/// A planted sidecar symlink is refused, and its target keeps its mode.
#[cfg(unix)]
#[test]
fn a_symlinked_sidecar_is_refused_and_its_target_left_alone() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("x.db");
    drop(SpentDay::open(db.to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap());
    let victim = dir.path().join("victim");
    std::fs::write(&victim, "not the database").unwrap();
    std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
    for sidecar in ["x.db-wal", "x.db-shm"] {
        let planted = dir.path().join(sidecar);
        let _ = std::fs::remove_file(&planted);
        std::os::unix::fs::symlink(&victim, &planted).unwrap();
        match SpentDay::open(db.to_string_lossy().into_owned(), "box".into(), Platform::Linux) {
            Err(spentday_core::SpentDayError::Storage(why)) => assert!(why.contains("symlink"), "{why}"),
            other => panic!("{sidecar}: expected a Storage error, got {:?}", other.err()),
        }
        std::fs::remove_file(&planted).unwrap();
    }
    assert_eq!(std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777, 0o644);
}

/// A database under a symlinked folder, such as macOS's /tmp, still opens,
/// and its sidecars land beside the real file.
#[cfg(unix)]
#[test]
fn a_symlinked_folder_still_works() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let m = SpentDay::open(link.join("x.db").to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap();
    m.heartbeat(
        1_000,
        Some(Activity {
            app_id: "slack".into(),
            app_name: "Slack".into(),
            web: None,
        }),
    )
    .unwrap();
    for file in ["x.db", "x.db-wal", "x.db-shm"] {
        let mode = std::fs::metadata(real.join(file)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{file}");
    }
}

/// The folder a symlink points at may have a name that is not UTF-8. SQLite
/// must open the file there, the one the core made private, and no other.
#[cfg(unix)]
#[test]
fn a_symlinked_folder_with_a_non_utf8_name_opens_the_same_file() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join(std::ffi::OsStr::from_bytes(b"real-\xff"));
    if let Err(e) = std::fs::create_dir(&real) {
        // APFS refuses names that are not UTF-8. The Linux run in
        // apps/linux/verify.sh covers this test.
        if cfg!(target_os = "macos") && e.raw_os_error() == Some(92) {
            eprintln!("skipped: this filesystem refuses non-UTF-8 names ({e})");
            return;
        }
        panic!("{e}");
    }
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let m = SpentDay::open(link.join("x.db").to_string_lossy().into_owned(), "box".into(), Platform::Linux).unwrap();
    m.heartbeat(
        1_000,
        Some(Activity {
            app_id: "slack".into(),
            app_name: "Slack".into(),
            web: None,
        }),
    )
    .unwrap();
    drop(m);
    let mut names: Vec<_> = std::fs::read_dir(root.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    names.sort();
    assert_eq!(names, [std::ffi::OsString::from("link"), real.file_name().unwrap().to_owned()]);
    let db = real.join("x.db");
    assert!(std::fs::metadata(&db).unwrap().len() > 0, "SQLite wrote another file");
    assert_eq!(std::fs::metadata(&db).unwrap().permissions().mode() & 0o777, 0o600);
}

/// A database of ours with no read bit is refused with the command that fixes
/// it. The core does not chmod it by path, because chmod would follow a symlink
/// swapped in. The command quotes the path, which here has a space and a `'`,
/// like the macOS default under "Application Support".
#[cfg(unix)]
#[test]
fn an_unreadable_database_of_ours_is_refused_with_a_command_that_fixes_it() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("Application Support").join("it's here");
    let db = dir.join("x.db");
    let path = db.to_string_lossy().into_owned();
    drop(SpentDay::open(path.clone(), "box".into(), Platform::Linux).unwrap());
    for mode in [0o200, 0o000] {
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(mode)).unwrap();
        if std::fs::File::open(&db).is_ok() {
            eprintln!("skipped: this user can read a {mode:o} file, as root can");
            return;
        }
        let e = SpentDay::open(path.clone(), "box".into(), Platform::Linux).err().unwrap().to_string();
        assert_eq!(e.matches("x.db").count(), 1, "names the file once: {e}");
        assert_eq!(std::fs::metadata(&db).unwrap().permissions().mode() & 0o777, mode, "{mode:o}");
        let command = e.split("Run ").nth(1).and_then(|rest| rest.split(", then try again").next()).unwrap();
        let ran = std::process::Command::new("sh").arg("-c").arg(command).status().unwrap();
        assert!(ran.success(), "{command}");
        assert_eq!(std::fs::metadata(&db).unwrap().permissions().mode() & 0o777, 0o600, "{command}");
        drop(SpentDay::open(path.clone(), "box".into(), Platform::Linux).unwrap());
    }
}

/// Another program that can write to the folder keeps putting an unreadable
/// file at a sidecar path and swapping a symlink in for it. The symlink's
/// target, a file of ours, never has its mode changed.
#[cfg(unix)]
#[test]
fn a_symlink_swapped_in_at_a_sidecar_keeps_its_target_mode() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("data");
    let db = dir.join("x.db");
    let path = db.to_string_lossy().into_owned();
    drop(SpentDay::open(path.clone(), "box".into(), Platform::Linux).unwrap());
    let victim = root.path().join("victim");
    std::fs::write(&victim, "keep").unwrap();
    std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
    // As root the unreadable file opens, so the repair this guards against is never tried.
    let probe = root.path().join("probe");
    std::fs::write(&probe, "").unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&probe).is_ok() {
        eprintln!("skipped: this user can read a 0 file, as root can");
        return;
    }

    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let attacker = {
        let (dir, victim, stop) = (dir.clone(), victim.clone(), stop.clone());
        std::thread::spawn(move || {
            let journal = dir.join("x.db-journal");
            let (plain, link) = (dir.join("plain"), dir.join("link"));
            while !stop.load(Ordering::Relaxed) {
                let _ = std::fs::write(&plain, "");
                let _ = std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o000));
                let _ = std::fs::rename(&plain, &journal);
                let _ = std::os::unix::fs::symlink(&victim, &link);
                let _ = std::fs::rename(&link, &journal);
            }
        })
    };
    for _ in 0..3000 {
        drop(SpentDay::open(path.clone(), "box".into(), Platform::Linux));
    }
    stop.store(true, Ordering::Relaxed);
    attacker.join().unwrap();
    assert_eq!(std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777, 0o644);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
}
