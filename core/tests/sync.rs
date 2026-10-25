use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use spentday_core::{
    Activity, DomainUsage, SpentDay, SpentDayError, Platform, SummaryQuery, SyncConfig, SyncReport,
    WebPage,
};

/// Starts spentday-server in-process on a random port and returns its URL.
fn start_server(dir: &tempfile::TempDir) -> String {
    let conn = spentday_server::open_db(&dir.path().join("server.db").to_string_lossy()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            spentday_server::serve(listener, conn).await.unwrap();
        });
    });
    url
}

type Hold = Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>;

/// Sits in front of a server, one thread per request. Fails pushes with 503
/// while `fail_push` is set, holds the next push or pull response until the
/// test releases it, and with `poison_next_pull` adds a bogus batch at cursor
/// `i64::MAX` to the next pull response, as a hostile server or anything
/// between it and the device could.
#[derive(Default)]
struct Proxy {
    fail_push: AtomicBool,
    hold_next_push: Hold,
    hold_next_pull: Hold,
    poison_next_pull: AtomicBool,
}

fn start_proxy(upstream: String) -> (String, Arc<Proxy>) {
    let control = Arc::new(Proxy::default());
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr().to_ip().unwrap());
    let shared = control.clone();
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        for request in server.incoming_requests() {
            let (agent, shared, upstream) = (agent.clone(), shared.clone(), upstream.clone());
            std::thread::spawn(move || forward(&agent, &shared, &upstream, request));
        }
    });
    (url, control)
}

fn forward(agent: &ureq::Agent, shared: &Proxy, upstream: &str, mut request: tiny_http::Request) {
    let target = format!("{upstream}{}", request.url());
    let auth = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.to_string())
        .unwrap_or_default();
    let is_push = *request.method() == tiny_http::Method::Post;
    let response = if is_push {
        if shared.fail_push.load(Ordering::SeqCst) {
            let _ = request.respond(tiny_http::Response::empty(503));
            return;
        }
        let mut body = String::new();
        request.as_reader().read_to_string(&mut body).unwrap();
        agent
            .post(&target)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .send(body)
    } else {
        agent.get(&target).header("Authorization", &auth).call()
    };
    let mut response = response.unwrap();
    let status = response.status().as_u16();
    let mut body = response
        .body_mut()
        .with_config()
        .limit(u64::MAX)
        .read_to_vec()
        .unwrap();
    if !is_push && shared.poison_next_pull.swap(false, Ordering::SeqCst) {
        let mut page: serde_json::Value = serde_json::from_slice(&body).unwrap();
        page["batches"].as_array_mut().unwrap().push(serde_json::json!({
            "cursor": i64::MAX, "device_id": "bogus",
            "nonce": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "ciphertext": "AAAA",
        }));
        page["head"] = i64::MAX.into();
        body = serde_json::to_vec(&page).unwrap();
    }
    let hold = if is_push {
        &shared.hold_next_push
    } else {
        &shared.hold_next_pull
    };
    if let Some((entered, release)) = hold.lock().unwrap().take() {
        entered.send(()).unwrap();
        release.recv().unwrap();
    }
    let _ = request.respond(tiny_http::Response::from_data(body).with_status_code(status));
}

fn open(dir: &tempfile::TempDir, file: &str, name: &str, platform: Platform) -> Arc<SpentDay> {
    SpentDay::open(
        dir.path().join(file).to_string_lossy().into_owned(),
        name.into(),
        platform,
    )
    .unwrap()
}

fn chrome(app_id: &str, url: Option<&str>) -> Activity {
    Activity {
        app_id: app_id.into(),
        app_name: "Google Chrome".into(),
        web: url.map(|u| WebPage {
            url: u.into(),
        }),
    }
}

fn day() -> SummaryQuery {
    SummaryQuery {
        start_ms: 0,
        end_ms: 200_000,
        bucket_ms: 100_000,
        device_id: None,
    }
}

fn counts(r: SyncReport) -> (u64, u64) {
    (r.pushed_spans, r.pulled_spans)
}

fn devices_of(m: &SpentDay, q: SummaryQuery) -> Vec<(String, i64)> {
    m.summary(q)
        .unwrap()
        .by_device
        .into_iter()
        .map(|d| (d.device.name, d.total_ms))
        .collect()
}

#[test]
fn two_devices_converge_on_one_summary() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let mac = open(&dir, "mac.db", "MacBook", Platform::Macos);
    let phone = open(&dir, "phone.db", "Pixel", Platform::Android);

    for at in (0..=60_000).step_by(5_000) {
        mac.heartbeat(
            at,
            Some(chrome(
                "com.google.Chrome",
                Some("https://www.youtube.com/watch?v=1"),
            )),
        )
        .unwrap();
    }
    mac.flush(60_000).unwrap();
    phone
        .web_hint(
            5_000,
            "com.android.chrome".into(),
            WebPage {
                url: "github.com/".into(),
            },
        )
        .unwrap();
    phone
        .record_interval(10_000, 40_000, chrome("com.android.chrome", None))
        .unwrap();

    let code = mac.new_sync_code();
    for m in [&mac, &phone] {
        m.set_sync_config(Some(SyncConfig {
            server_url: server.clone(),
            sync_code: code.clone(),
        }))
        .unwrap();
    }
    assert_eq!(counts(mac.sync_now().unwrap()), (1, 0));
    assert_eq!(counts(phone.sync_now().unwrap()), (1, 1));
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 1));

    let summary = mac.summary(day()).unwrap();
    assert_eq!(phone.summary(day()).unwrap(), summary);
    assert_eq!(summary.total_ms, 90_000);
    assert_eq!(summary.buckets, vec![90_000, 0]);
    assert_eq!(
        devices_of(&mac, day()),
        vec![("MacBook".to_owned(), 60_000), ("Pixel".to_owned(), 30_000)]
    );
    assert_eq!(summary.apps.len(), 1);
    assert_eq!(
        (
            summary.apps[0].app_key.as_str(),
            summary.apps[0].app_name.as_str(),
            summary.apps[0].total_ms
        ),
        ("chrome", "Chrome", 90_000)
    );
    let expected_sites = vec![
        DomainUsage {
            domain: "youtube.com".into(),
            total_ms: 60_000,
        },
        DomainUsage {
            domain: "github.com".into(),
            total_ms: 30_000,
        },
    ];
    assert_eq!(summary.apps[0].domains, expected_sites);
    assert_eq!(summary.sites, expected_sites);

    let phone_only = SummaryQuery {
        device_id: Some(phone.device().id),
        ..day()
    };
    assert_eq!(
        devices_of(&mac, phone_only),
        vec![("Pixel".to_owned(), 30_000)]
    );
    let known: Vec<_> = mac
        .devices()
        .unwrap()
        .into_iter()
        .map(|d| (d.name, d.platform, d.last_seen_ms))
        .collect();
    assert_eq!(
        known,
        vec![
            ("MacBook".to_owned(), Platform::Macos, 60_000),
            ("Pixel".to_owned(), Platform::Android, 40_000)
        ]
    );

    // Nothing new: both directions are no-ops and nothing double counts.
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 0));
    assert_eq!(counts(phone.sync_now().unwrap()), (0, 0));
    assert_eq!(mac.summary(day()).unwrap(), summary);

    // The phone re-reports a longer interval: the span is replaced, not added.
    phone
        .record_interval(10_000, 50_000, chrome("com.android.chrome", None))
        .unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (1, 0));
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 1));
    assert_eq!(
        devices_of(&mac, day()),
        vec![("MacBook".to_owned(), 60_000), ("Pixel".to_owned(), 40_000)]
    );
    assert_eq!(mac.summary(day()).unwrap().total_ms, 100_000);

    // A sync from scratch (fresh cursor) re-pulls everything and converges to the same totals.
    mac.set_sync_config(None).unwrap();
    mac.set_sync_config(Some(SyncConfig {
        server_url: server.clone(),
        sync_code: code.clone(),
    }))
    .unwrap();
    assert_eq!(counts(mac.sync_now().unwrap()), (1, 2));
    assert_eq!(mac.summary(day()).unwrap().total_ms, 100_000);
}

#[test]
fn time_cut_by_away_since_leaves_other_devices_too() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let mac = open(&dir, "mac.db", "MacBook", Platform::Macos);
    let phone = open(&dir, "phone.db", "Pixel", Platform::Android);
    let code = mac.new_sync_code();
    for m in [&mac, &phone] {
        m.set_sync_config(Some(SyncConfig {
            server_url: server.clone(),
            sync_code: code.clone(),
        }))
        .unwrap();
    }
    for at in (0..=180_000).step_by(5_000) {
        mac.heartbeat(at, Some(chrome("com.google.Chrome", None)))
            .unwrap();
    }
    assert_eq!(counts(mac.sync_now().unwrap()), (1, 0));
    assert_eq!(counts(phone.sync_now().unwrap()), (0, 1));
    assert_eq!(
        devices_of(&phone, day()),
        vec![("MacBook".to_owned(), 180_000)]
    );

    mac.away_since(30_000, 180_000).unwrap();
    assert_eq!(counts(mac.sync_now().unwrap()), (1, 0));
    assert_eq!(counts(phone.sync_now().unwrap()), (0, 1));
    assert_eq!(
        devices_of(&phone, day()),
        vec![("MacBook".to_owned(), 30_000)]
    );
}

#[test]
fn a_cut_across_spans_split_while_idle_reaches_other_devices() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let mac = open(&dir, "mac.db", "MacBook", Platform::Macos);
    let phone = open(&dir, "phone.db", "Pixel", Platform::Android);
    join(&server, &mac.new_sync_code(), &[&mac, &phone]);
    // No input after 20 s. The page moves on to another site by itself at 60 s.
    for at in (0..=200_000).step_by(5_000) {
        let url = if at < 60_000 { "https://a.example/" } else { "https://b.example/" };
        mac.heartbeat(at, Some(chrome("com.google.Chrome", Some(url))))
            .unwrap();
    }
    assert_eq!(counts(mac.sync_now().unwrap()), (2, 0));
    assert_eq!(counts(phone.sync_now().unwrap()), (0, 2));

    mac.away_since(20_000, 200_000).unwrap();
    assert_eq!(mac.summary(day()).unwrap().total_ms, 20_000);
    assert_eq!(counts(mac.sync_now().unwrap()), (2, 0));
    assert_eq!(counts(phone.sync_now().unwrap()), (0, 2));
    assert_eq!(
        devices_of(&phone, day()),
        vec![("MacBook".to_owned(), 20_000)]
    );
}

#[test]
fn sync_config_never_pairs_one_accounts_server_with_anothers_code() {
    let dir = tempfile::tempdir().unwrap();
    let reader = open(&dir, "a.db", "A", Platform::Linux);
    let writer = open(&dir, "a.db", "A", Platform::Linux);
    let accounts = [
        SyncConfig {
            server_url: "https://one.example".into(),
            sync_code: reader.new_sync_code(),
        },
        SyncConfig {
            server_url: "https://two.example".into(),
            sync_code: reader.new_sync_code(),
        },
    ];
    let done = AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            for i in 0..2_000 {
                writer
                    .set_sync_config(Some(accounts[i % 2].clone()))
                    .unwrap();
            }
            done.store(true, Ordering::SeqCst);
        });
        while !done.load(Ordering::SeqCst) {
            if let Some(config) = reader.sync_config().unwrap() {
                assert!(accounts.contains(&config), "torn read: {config:?}");
            }
        }
    });
}

#[test]
fn a_huge_device_name_is_clipped_and_still_syncs() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let m = open(&dir, "a.db", &"é".repeat(4 * 1024 * 1024), Platform::Linux);
    let name = m.device().name;
    assert!(name == "é".repeat(128), "kept {} bytes", name.len());
    m.record_interval(0, 10_000, chrome("google-chrome", None))
        .unwrap();
    m.set_sync_config(Some(SyncConfig {
        server_url: server,
        sync_code: m.new_sync_code(),
    }))
    .unwrap();
    assert_eq!(counts(m.sync_now().unwrap()), (1, 0));
}

#[test]
fn concurrent_syncs_push_each_batch_once() {
    let dir = tempfile::tempdir().unwrap();
    let (server, proxy) = start_proxy(start_server(&dir));
    let a = open(&dir, "a.db", "A", Platform::Macos);
    let b = open(&dir, "b.db", "B", Platform::Linux);
    join(&server, &a.new_sync_code(), &[&a, &b]);
    a.record_interval(0, 10_000, chrome("google-chrome", None))
        .unwrap();

    let (entered, held) = mpsc::channel();
    let (release, released) = mpsc::channel();
    *proxy.hold_next_push.lock().unwrap() = Some((entered, released));
    std::thread::scope(|s| {
        let first = s.spawn(|| a.sync_now().unwrap());
        held.recv().unwrap();
        // The UI syncs while a background sync's upload is in flight.
        let second = s.spawn(|| a.sync_now().unwrap());
        std::thread::sleep(std::time::Duration::from_millis(300));
        release.send(()).unwrap();
        let pushed = [first.join().unwrap(), second.join().unwrap()].map(|r| counts(r).0);
        assert_eq!(pushed, [1, 0]);
    });
    assert_eq!(counts(b.sync_now().unwrap()), (0, 1));
}

#[test]
fn another_code_sees_nothing_and_typos_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let a = open(&dir, "a.db", "A", Platform::Linux);
    let b = open(&dir, "b.db", "B", Platform::Linux);
    a.record_interval(0, 10_000, chrome("google-chrome", None))
        .unwrap();
    b.record_interval(0, 20_000, chrome("google-chrome", None))
        .unwrap();

    let code_a = a.new_sync_code();
    let code_b = b.new_sync_code();
    a.set_sync_config(Some(SyncConfig {
        server_url: server.clone(),
        sync_code: code_a.clone(),
    }))
    .unwrap();
    b.set_sync_config(Some(SyncConfig {
        server_url: server.clone(),
        sync_code: code_b,
    }))
    .unwrap();
    assert_eq!(counts(a.sync_now().unwrap()), (1, 0));
    assert_eq!(counts(b.sync_now().unwrap()), (1, 0));
    assert_eq!(devices_of(&b, day()), vec![("B".to_owned(), 20_000)]);

    // The last symbol holds only check bits. Flipping one changes the check
    // and not the secret, so the code can never verify.
    const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let last = code_a.chars().last().unwrap();
    let flipped = CROCKFORD.as_bytes()[CROCKFORD.find(last).unwrap() ^ 1] as char;
    let typo = format!("{}{flipped}", &code_a[..code_a.len() - 1]);
    let err = b.set_sync_config(Some(SyncConfig {
        server_url: server,
        sync_code: typo,
    }));
    assert!(matches!(err, Err(SpentDayError::InvalidSyncCode)), "{err:?}");
}

fn device_total(m: &SpentDay, device_id: &str) -> i64 {
    m.summary(SummaryQuery {
        start_ms: 0,
        end_ms: 100_000,
        bucket_ms: 0,
        device_id: Some(device_id.into()),
    })
    .unwrap()
    .total_ms
}

fn join(server: &str, code: &str, devices: &[&Arc<SpentDay>]) {
    for m in devices {
        m.set_sync_config(Some(SyncConfig {
            server_url: server.into(),
            sync_code: code.into(),
        }))
        .unwrap();
    }
}

#[test]
fn two_handles_on_one_database_never_reuse_a_rev() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let path = dir.path().join("a.db").to_string_lossy().into_owned();
    let a1 = SpentDay::open(path.clone(), "A".into(), Platform::Android).unwrap();
    let a2 = SpentDay::open(path, "A".into(), Platform::Android).unwrap();
    let b = open(&dir, "b.db", "B", Platform::Linux);
    let code = a1.new_sync_code();
    join(&server, &code, &[&a1, &b]);
    let a = a1.device().id;
    let maps = || chrome("com.google.android.apps.maps", None);

    for i in 0..5 {
        a1.record_interval(i * 1_000, i * 1_000 + 100, maps())
            .unwrap();
    }
    assert_eq!(counts(a1.sync_now().unwrap()), (5, 0));
    a2.record_interval(0, 900, maps()).unwrap();
    assert_eq!(counts(a1.sync_now().unwrap()), (1, 0));
    assert_eq!(counts(b.sync_now().unwrap()), (0, 6));
    assert_eq!(
        (device_total(&a1, &a), device_total(&b, &a)),
        (1_300, 1_300)
    );

    a1.record_interval(1_000, 1_500, maps()).unwrap();
    a2.record_interval(10_000, 11_000, maps()).unwrap();
    a1.record_interval(2_000, 2_700, maps()).unwrap();
    assert_eq!(counts(a2.sync_now().unwrap()), (3, 0));
    assert_eq!(counts(b.sync_now().unwrap()), (0, 3));
    assert_eq!(
        (device_total(&a1, &a), device_total(&b, &a)),
        (3_300, 3_300)
    );
}

#[test]
fn reingested_intervals_replace_their_old_pieces_on_every_device() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let path = dir.path().join("phone.db").to_string_lossy().into_owned();
    let phone = SpentDay::open(path.clone(), "Pixel".into(), Platform::Android).unwrap();
    let mac = open(&dir, "mac.db", "MacBook", Platform::Macos);
    let code = phone.new_sync_code();
    join(&server, &code, &[&phone, &mac]);
    let id = phone.device().id;
    let hint = |m: &SpentDay, at: i64, url: &str| {
        m.web_hint(
            at,
            "com.android.chrome".into(),
            WebPage {
                url: url.into(),
            },
        )
        .unwrap()
    };

    hint(&phone, 20_000, "youtube.com/");
    hint(&phone, 60_000, "github.com/");
    phone
        .record_interval(0, 100_000, chrome("com.android.chrome", None))
        .unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (3, 0));
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 3));
    assert_eq!(device_total(&mac, &id), 100_000);

    // A week later the hints inside the interval are pruned on reopen, and
    // UsageStats reports the same interval again: one piece instead of three.
    hint(&phone, 100_000 + 7 * 24 * 3_600_000 + 1, "example.com/");
    drop(phone);
    let phone = SpentDay::open(path, "Pixel".into(), Platform::Android).unwrap();
    phone
        .record_interval(0, 100_000, chrome("com.android.chrome", None))
        .unwrap();
    assert_eq!(device_total(&phone, &id), 100_000);
    assert_eq!(counts(phone.sync_now().unwrap()), (3, 0));
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 3));
    assert_eq!(device_total(&mac, &id), 100_000);

    // A shortened re-report drops the pieces past its new end.
    let later = 200_000 + 7 * 24 * 3_600_000;
    hint(&phone, later + 20_000, "youtube.com/");
    hint(&phone, later + 60_000, "github.com/");
    phone
        .record_interval(later, later + 100_000, chrome("com.android.chrome", None))
        .unwrap();
    phone
        .record_interval(later, later + 50_000, chrome("com.android.chrome", None))
        .unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (3, 0));
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 3));
    for m in [&phone, &mac] {
        let sum = m
            .summary(SummaryQuery {
                start_ms: later,
                end_ms: later + 200_000,
                bucket_ms: 0,
                device_id: Some(id.clone()),
            })
            .unwrap();
        assert_eq!(sum.total_ms, 50_000);
        assert_eq!(
            sum.sites,
            vec![
                DomainUsage {
                    domain: "youtube.com".into(),
                    total_ms: 30_000
                },
                DomainUsage {
                    domain: "example.com".into(),
                    total_ms: 20_000
                },
            ]
        );
    }
}

#[test]
fn a_retracted_interval_drops_its_time_on_every_device() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let phone = open(&dir, "phone.db", "Pixel", Platform::Android);
    let mac = open(&dir, "mac.db", "MacBook", Platform::Macos);
    join(&server, &phone.new_sync_code(), &[&phone, &mac]);
    let id = phone.device().id;
    let chrome = || chrome("com.android.chrome", None);

    phone
        .web_hint(
            10_000,
            "com.android.chrome".into(),
            WebPage {
                url: "https://youtube.com/".into(),
            },
        )
        .unwrap();
    phone.record_interval(1_000, 30_000, chrome()).unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (2, 0));
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 2));
    assert_eq!(device_total(&mac, &id), 29_000);

    phone.record_interval(1_000, 1_000, chrome()).unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (2, 0));
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 2));
    assert_eq!((device_total(&phone, &id), device_total(&mac, &id)), (0, 0));

    phone.record_interval(1_000, 1_000, chrome()).unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (0, 0));

    phone.record_interval(1_000, 20_000, chrome()).unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (2, 0));
    assert_eq!(counts(mac.sync_now().unwrap()), (0, 2));
    assert_eq!(
        (device_total(&phone, &id), device_total(&mac, &id)),
        (19_000, 19_000)
    );
}

#[test]
fn retracting_an_unknown_interval_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let phone = open(&dir, "phone.db", "Pixel", Platform::Android);
    join(&server, &phone.new_sync_code(), &[&phone]);
    let id = phone.device().id;
    let maps = || chrome("com.google.android.apps.maps", None);

    phone.record_interval(1_000, 30_000, maps()).unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (1, 0));

    phone.record_interval(5_000, 5_000, maps()).unwrap();
    phone.record_interval(5_000, 2_000, maps()).unwrap();
    phone
        .record_interval(1_000, 1_000, chrome("com.android.chrome", None))
        .unwrap();
    assert_eq!(counts(phone.sync_now().unwrap()), (0, 0));
    assert_eq!(device_total(&phone, &id), 29_000);
}

#[test]
fn spans_with_huge_urls_and_app_names_still_sync() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let a = open(&dir, "a.db", "A", Platform::Macos);
    let b = open(&dir, "b.db", "B", Platform::Linux);
    join(&server, &a.new_sync_code(), &[&a, &b]);
    for i in 0..2_000 {
        let activity = Activity {
            app_name: "\"".repeat(4_000),
            web: Some(WebPage {
                url: format!("https://example.com/?q={i}&t={}", "a".repeat(4_000)),
            }),
            ..chrome("com.google.Chrome", None)
        };
        a.record_interval(i * 1_000, i * 1_000 + 500, activity)
            .unwrap();
    }
    assert_eq!(counts(a.sync_now().unwrap()), (2_000, 0));
    assert_eq!(counts(b.sync_now().unwrap()), (0, 2_000));
    let sum = b
        .summary(SummaryQuery {
            start_ms: 0,
            end_ms: 2_000_000,
            bucket_ms: 0,
            device_id: None,
        })
        .unwrap();
    assert_eq!(
        sum.sites,
        vec![DomainUsage {
            domain: "example.com".into(),
            total_ms: 1_000_000
        }]
    );
    let span = &b.recent_spans(1).unwrap()[0];
    assert_eq!(
        (span.app_name.len(), span.domain.as_deref()),
        (512, Some("example.com"))
    );
}

#[test]
fn a_failing_push_still_pulls() {
    let dir = tempfile::tempdir().unwrap();
    let (server, proxy) = start_proxy(start_server(&dir));
    let a = open(&dir, "a.db", "A", Platform::Macos);
    let b = open(&dir, "b.db", "B", Platform::Linux);
    join(&server, &a.new_sync_code(), &[&a, &b]);
    a.record_interval(0, 10_000, chrome("google-chrome", None))
        .unwrap();
    b.record_interval(0, 20_000, chrome("google-chrome", None))
        .unwrap();
    assert_eq!(counts(b.sync_now().unwrap()), (1, 0));

    proxy.fail_push.store(true, Ordering::SeqCst);
    let err = a.sync_now();
    assert!(
        matches!(err, Err(SpentDayError::Server { status: 503, .. })),
        "{err:?}"
    );
    assert_eq!(
        devices_of(&a, day()),
        vec![("B".to_owned(), 20_000), ("A".to_owned(), 10_000)]
    );

    proxy.fail_push.store(false, Ordering::SeqCst);
    assert_eq!(counts(a.sync_now().unwrap()), (1, 0));
    assert_eq!(counts(b.sync_now().unwrap()), (0, 1));
}

#[test]
fn a_new_device_pulls_more_than_10_mib_of_history() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_server(&dir);
    let a = open(&dir, "a.db", "A", Platform::Macos);
    let b = open(&dir, "b.db", "B", Platform::Linux);
    join(&server, &a.new_sync_code(), &[&a, &b]);
    // Each span is near 2.4 KB of JSON: quotes in its app id and name double
    // in size when escaped, and its domain is near 200 bytes.
    for i in 0..5_000 {
        let domain = format!(
            "{i:05}{}.{}.{}.example.com",
            "a".repeat(55),
            "b".repeat(60),
            "c".repeat(60)
        );
        let activity = Activity {
            app_id: "\"".repeat(512),
            app_name: "\"".repeat(512),
            web: Some(WebPage {
                url: format!("https://{domain}/"),
            }),
        };
        a.record_interval(i * 1_000, i * 1_000 + 500, activity)
            .unwrap();
    }
    assert_eq!(counts(a.sync_now().unwrap()), (5_000, 0));
    assert_eq!(counts(b.sync_now().unwrap()), (0, 5_000));
    let sum = b
        .summary(SummaryQuery {
            start_ms: 0,
            end_ms: 5_000_000,
            bucket_ms: 0,
            device_id: None,
        })
        .unwrap();
    assert_eq!((sum.by_device.len(), sum.total_ms), (1, 2_500_000));
}

#[test]
fn a_sync_in_flight_writes_nothing_after_the_account_changes() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = start_server(&dir);
    let (proxied, proxy) = start_proxy(upstream.clone());
    let c = open(&dir, "c.db", "C", Platform::Linux);
    let x = open(&dir, "x.db", "X", Platform::Linux);
    let (new_code, old_code) = (c.new_sync_code(), x.new_sync_code());
    // The new account's only batch gets server cursor 1, the old account's 2.
    join(&upstream, &new_code, &[&c]);
    c.record_interval(0, 30_000, chrome("google-chrome", None))
        .unwrap();
    assert_eq!(counts(c.sync_now().unwrap()), (1, 0));
    join(&upstream, &old_code, &[&x]);
    x.record_interval(0, 10_000, chrome("google-chrome", None))
        .unwrap();
    assert_eq!(counts(x.sync_now().unwrap()), (1, 0));

    let a = open(&dir, "a.db", "A", Platform::Macos);
    join(&proxied, &old_code, &[&a]);
    let (entered, held) = mpsc::channel();
    let (release, released) = mpsc::channel();
    *proxy.hold_next_pull.lock().unwrap() = Some((entered, released));
    let syncing = {
        let a = a.clone();
        std::thread::spawn(move || a.sync_now().unwrap())
    };
    held.recv().unwrap();
    join(&upstream, &new_code, &[&a]);
    release.send(()).unwrap();
    assert_eq!(counts(syncing.join().unwrap()), (0, 0));

    assert_eq!(counts(a.sync_now().unwrap()), (0, 1));
    assert_eq!(devices_of(&a, day()), vec![("C".to_owned(), 30_000)]);
}

/// One bogus batch at cursor `i64::MAX` used to move the stored cursor there
/// for good, so the device asked for batches after it forever and never
/// received anything again, even from an honest server.
#[test]
fn a_poisoned_pull_cursor_recovers_on_the_next_sync() {
    let dir = tempfile::tempdir().unwrap();
    let (server, proxy) = start_proxy(start_server(&dir));
    let a = open(&dir, "a.db", "A", Platform::Macos);
    let b = open(&dir, "b.db", "B", Platform::Linux);
    join(&server, &a.new_sync_code(), &[&a, &b]);
    a.record_interval(0, 10_000, chrome("google-chrome", None))
        .unwrap();
    assert_eq!(counts(a.sync_now().unwrap()), (1, 0));

    proxy.poison_next_pull.store(true, Ordering::SeqCst);
    let poisoned = b.sync_now().unwrap();
    assert_eq!((poisoned.pulled_spans, poisoned.skipped_batches), (1, 1));

    a.record_interval(20_000, 50_000, chrome("google-chrome", None))
        .unwrap();
    assert_eq!(counts(a.sync_now().unwrap()), (1, 0));
    // The server's head is 2, behind the stored cursor, so B reads the space
    // again from the start. Its first batch merges again as a no-op.
    let recovered = b.sync_now().unwrap();
    assert_eq!((recovered.pulled_spans, recovered.skipped_batches), (2, 0));
    assert_eq!(devices_of(&b, day()), vec![("A".to_owned(), 40_000)]);
    assert_eq!(counts(b.sync_now().unwrap()), (0, 0));
}
