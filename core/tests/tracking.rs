use std::sync::Arc;

use spentday_core::{Activity, DomainUsage, SpentDay, SpentDayError, Platform, SummaryQuery, WebPage};

fn open(dir: &tempfile::TempDir, platform: Platform) -> Arc<SpentDay> {
    SpentDay::open(
        dir.path().join("spentday.db").to_string_lossy().into_owned(),
        "test box".into(),
        platform,
    )
    .unwrap()
}

fn app(app_id: &str, app_name: &str) -> Activity {
    Activity {
        app_id: app_id.into(),
        app_name: app_name.into(),
        web: None,
    }
}

fn browsing(app_id: &str, url: &str) -> Activity {
    Activity {
        web: Some(WebPage {
            url: url.into(),
        }),
        ..app(app_id, "Browser")
    }
}

fn page(url: &str) -> WebPage {
    WebPage {
        url: url.into(),
    }
}

fn query(start_ms: i64, end_ms: i64, bucket_ms: i64) -> SummaryQuery {
    SummaryQuery {
        start_ms,
        end_ms,
        bucket_ms,
        device_id: None,
    }
}

/// `(start, end, app_id, domain)` for every span, oldest first.
fn spans(m: &SpentDay) -> Vec<(i64, i64, String, Option<String>)> {
    let mut rows: Vec<_> = m
        .recent_spans(1000)
        .unwrap()
        .into_iter()
        .map(|s| (s.start_ms, s.end_ms, s.app_id, s.domain))
        .collect();
    rows.reverse();
    rows
}

fn domains(pairs: &[(&str, i64)]) -> Vec<DomainUsage> {
    pairs
        .iter()
        .map(|&(domain, total_ms)| DomainUsage {
            domain: domain.into(),
            total_ms,
        })
        .collect()
}

fn s(
    start: i64,
    end: i64,
    app_id: &str,
    domain: Option<&str>,
) -> (i64, i64, String, Option<String>) {
    (start, end, app_id.into(), domain.map(Into::into))
}

#[test]
fn heartbeats_merge_split_on_change_gap_and_away() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    let terminal = app("org.gnome.Terminal", "Terminal");
    let slack = app("slack", "Slack");

    for at in [0, 5_000, 10_000] {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    m.heartbeat(12_000, Some(slack.clone())).unwrap();
    m.heartbeat(17_000, Some(slack.clone())).unwrap();
    // 43 s of silence: the slack span stops at its last heartbeat.
    m.heartbeat(60_000, Some(slack.clone())).unwrap();
    m.heartbeat(65_000, Some(slack.clone())).unwrap();
    m.heartbeat(70_000, None).unwrap();
    m.heartbeat(75_000, Some(slack.clone())).unwrap();
    m.flush(80_000).unwrap();
    m.flush(90_000).unwrap();

    assert_eq!(
        spans(&m),
        vec![
            s(0, 12_000, "org.gnome.Terminal", None),
            s(12_000, 17_000, "slack", None),
            s(60_000, 70_000, "slack", None),
            s(75_000, 80_000, "slack", None),
        ]
    );

    let sum = m.summary(query(0, 100_000, 50_000)).unwrap();
    assert_eq!(sum.total_ms, 32_000);
    assert_eq!(sum.buckets, vec![17_000, 15_000]);
    let apps: Vec<_> = sum
        .apps
        .iter()
        .map(|a| {
            (
                a.app_key.as_str(),
                a.app_name.as_str(),
                a.is_browser,
                a.total_ms,
            )
        })
        .collect();
    assert_eq!(
        apps,
        vec![
            ("slack", "Slack", false, 20_000),
            ("org.gnome.Terminal", "Terminal", false, 12_000)
        ]
    );
    assert_eq!(sum.by_device.len(), 1);
    assert_eq!(sum.by_device[0].total_ms, 32_000);
    assert_eq!(sum.by_device[0].device, m.device());
}

#[test]
fn heartbeats_from_the_past_never_overlap() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Macos);
    let terminal = app("com.apple.Terminal", "Terminal");
    // The wall clock steps back 60 s mid-session.
    for at in (0..=120_000).step_by(5_000) {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    for at in (60_000..=180_000).step_by(5_000) {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    m.flush(180_000).unwrap();
    assert_eq!(spans(&m), vec![s(0, 180_000, "com.apple.Terminal", None)]);
    assert_eq!(m.summary(query(0, 1_000_000, 0)).unwrap().total_ms, 180_000);

    // A focus callback delivered late, stamped before the last heartbeat.
    let slack = app("com.tinyspeck.slackmacgap", "Slack");
    for at in [200_000, 205_000, 210_000] {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    m.heartbeat(207_000, Some(slack.clone())).unwrap();
    m.heartbeat(215_000, Some(slack)).unwrap();
    m.flush(215_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(0, 180_000, "com.apple.Terminal", None),
            s(200_000, 210_000, "com.apple.Terminal", None),
            s(210_000, 215_000, "com.tinyspeck.slackmacgap", None),
        ]
    );
    assert_eq!(m.summary(query(0, 1_000_000, 0)).unwrap().total_ms, 195_000);
}

#[test]
fn away_since_cuts_the_open_span_back_to_the_last_input() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Macos);
    let terminal = app("com.apple.Terminal", "Terminal");
    for at in (0..=200_000).step_by(5_000) {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    m.away_since(20_000, 200_000).unwrap();
    // Away: the next heartbeat opens a new span instead of extending the cut one.
    m.heartbeat(205_000, Some(terminal.clone())).unwrap();
    m.heartbeat(210_000, Some(terminal.clone())).unwrap();
    // Last input before the span opened: the span keeps no time at all.
    m.away_since(100_000, 215_000).unwrap();
    for at in [300_000, 305_000, 310_000] {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    // Last input after the last heartbeat: never extends past it.
    m.away_since(400_000, 400_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(0, 20_000, "com.apple.Terminal", None),
            s(100_000, 100_000, "com.apple.Terminal", None),
            s(300_000, 310_000, "com.apple.Terminal", None),
        ]
    );
    assert_eq!(m.summary(query(0, 1_000_000, 0)).unwrap().total_ms, 30_000);
}

/// Chrome showing `site`, as macOS reads it with AppleScript.
fn showing(site: &str) -> Activity {
    browsing("com.google.Chrome", &format!("https://{site}/"))
}

/// Heartbeats through 200 s with no input after 20 s, while the page moves on
/// to another site by itself at 60 s.
fn idle_while_the_page_changes(m: &SpentDay) {
    for at in (0..=200_000).step_by(5_000) {
        let site = if at < 60_000 { "a.example" } else { "b.example" };
        m.heartbeat(at, Some(showing(site))).unwrap();
    }
}

#[test]
fn away_since_cuts_spans_a_site_change_split_while_idle() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    idle_while_the_page_changes(&m);
    m.away_since(20_000, 200_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(0, 20_000, "com.google.Chrome", Some("a.example")),
            s(20_000, 20_000, "com.google.Chrome", Some("b.example")),
        ]
    );
    assert_eq!(m.summary(query(0, 1_000_000, 0)).unwrap().total_ms, 20_000);
    assert_eq!(m.devices().unwrap()[0].last_seen_ms, 20_000);
}

#[test]
fn heartbeats_after_away_since_start_a_new_span_at_now() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    idle_while_the_page_changes(&m);
    m.away_since(20_000, 200_000).unwrap();
    for at in (200_000..=230_000).step_by(5_000) {
        m.heartbeat(at, Some(showing("b.example"))).unwrap();
    }
    m.flush(230_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(0, 20_000, "com.google.Chrome", Some("a.example")),
            s(20_000, 20_000, "com.google.Chrome", Some("b.example")),
            s(200_000, 230_000, "com.google.Chrome", Some("b.example")),
        ]
    );
    assert_eq!(m.summary(query(0, 1_000_000, 0)).unwrap().total_ms, 50_000);
}

#[test]
fn away_since_cuts_a_browser_span_split_by_a_no_page_hint_while_idle() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    let chrome = app("google-chrome", "Chrome");
    m.web_hint(0, "chrome".into(), page("https://youtube.com/"))
        .unwrap();
    m.web_cleared(50_000, "chrome".into()).unwrap();
    for at in (0..=200_000).step_by(5_000) {
        m.heartbeat(at, Some(chrome.clone())).unwrap();
    }
    m.away_since(20_000, 200_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(0, 20_000, "google-chrome", Some("youtube.com")),
            s(20_000, 20_000, "google-chrome", None),
        ]
    );
    let sum = m.summary(query(0, 1_000_000, 0)).unwrap();
    assert_eq!(sum.total_ms, 20_000);
    assert_eq!(sum.apps[0].domains, domains(&[("youtube.com", 20_000)]));
}

#[test]
fn away_since_before_the_open_span_cuts_earlier_pieces_and_keeps_intervals() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    for at in (0..=200_000).step_by(5_000) {
        m.heartbeat(at, Some(showing(&format!("step{}.example", at / 60_000))))
            .unwrap();
    }
    m.record_interval(0, 150_000, app("org.example.Sync", "Sync"))
        .unwrap();
    m.away_since(90_000, 200_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(0, 60_000, "com.google.Chrome", Some("step0.example")),
            s(0, 150_000, "org.example.Sync", None),
            s(60_000, 90_000, "com.google.Chrome", Some("step1.example")),
            s(90_000, 90_000, "com.google.Chrome", Some("step2.example")),
            s(90_000, 90_000, "com.google.Chrome", Some("step3.example")),
        ]
    );
    assert_eq!(m.summary(query(0, 1_000_000, 0)).unwrap().total_ms, 240_000);
    assert_eq!(m.devices().unwrap()[0].last_seen_ms, 150_000);
}

#[test]
fn a_clock_step_back_after_flush_or_away_never_overlaps() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    let terminal = app("org.gnome.Terminal", "Terminal");
    for at in (0..=60_000).step_by(5_000) {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    m.flush(60_000).unwrap();
    for at in (30_000..=90_000).step_by(5_000) {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    m.flush(90_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(0, 60_000, "org.gnome.Terminal", None),
            s(60_000, 90_000, "org.gnome.Terminal", None),
        ]
    );
    assert_eq!(m.summary(query(0, 1_000_000, 0)).unwrap().total_ms, 90_000);

    // After away_since the floor is the cut end, not the time it cut.
    for at in (100_000..=200_000).step_by(5_000) {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    m.away_since(120_000, 200_000).unwrap();
    for at in (110_000..=130_000).step_by(5_000) {
        m.heartbeat(at, Some(terminal.clone())).unwrap();
    }
    m.flush(130_000).unwrap();
    assert_eq!(m.summary(query(0, 1_000_000, 0)).unwrap().total_ms, 120_000);
    assert_eq!(
        spans(&m)[2..],
        [
            s(100_000, 120_000, "org.gnome.Terminal", None),
            s(120_000, 130_000, "org.gnome.Terminal", None),
        ]
    );
}

#[test]
fn a_clock_step_back_before_going_away_never_overlaps() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    m.heartbeat(0, Some(app("a", "A"))).unwrap();
    m.heartbeat(20_000, Some(app("a", "A"))).unwrap();
    // The clock goes back, then the collector reports idle. No flush.
    m.heartbeat(10_000, None).unwrap();
    m.heartbeat(10_000, Some(app("b", "B"))).unwrap();
    m.heartbeat(20_000, Some(app("b", "B"))).unwrap();
    assert_eq!(m.summary(query(0, 20_000, 0)).unwrap().total_ms, 20_000);
}

#[test]
fn open_span_survives_reopen_and_device_id_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let first = open(&dir, Platform::Linux);
    let id = first.device().id;
    first.heartbeat(0, Some(app("firefox", "Firefox"))).unwrap();
    first
        .heartbeat(5_000, Some(app("firefox", "Firefox")))
        .unwrap();
    drop(first);

    let path = dir.path().join("spentday.db").to_string_lossy().into_owned();
    let second = SpentDay::open(path, "renamed".into(), Platform::Linux).unwrap();
    second
        .heartbeat(10_000, Some(app("firefox", "Firefox")))
        .unwrap();
    assert_eq!(spans(&second), vec![s(0, 10_000, "firefox", None)]);
    let device = second.device();
    assert_eq!(
        (
            device.id,
            device.name.as_str(),
            device.platform,
            device.last_seen_ms
        ),
        (id, "renamed", Platform::Linux, 10_000)
    );
}

#[test]
fn browser_heartbeats_take_fresh_hints_only() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    let firefox = app("org.mozilla.firefox", "Firefox");

    m.web_hint(
        1_000,
        "firefox".into(),
        page("https://www.youtube.com/watch?v=1"),
    )
    .unwrap();
    m.heartbeat(30_000, Some(firefox.clone())).unwrap();
    m.heartbeat(35_000, Some(firefox.clone())).unwrap();
    // A new page splits the span at the next heartbeat.
    m.web_hint(36_000, "firefox".into(), page("https://github.com/"))
        .unwrap();
    m.heartbeat(40_000, Some(firefox.clone())).unwrap();
    // 100 s after the last hint it is stale, so time is the browser's but no site's.
    m.heartbeat(45_000, Some(firefox.clone())).unwrap();
    m.flush(50_000).unwrap();
    m.heartbeat(140_000, Some(firefox.clone())).unwrap();
    m.flush(145_000).unwrap();

    assert_eq!(
        spans(&m),
        vec![
            s(30_000, 40_000, "org.mozilla.firefox", Some("youtube.com")),
            s(40_000, 50_000, "org.mozilla.firefox", Some("github.com")),
            s(140_000, 145_000, "org.mozilla.firefox", None),
        ]
    );
    let sum = m.summary(query(0, 200_000, 200_000)).unwrap();
    assert_eq!(sum.apps.len(), 1);
    assert_eq!(
        (
            sum.apps[0].app_key.as_str(),
            sum.apps[0].is_browser,
            sum.apps[0].total_ms
        ),
        ("firefox", true, 25_000)
    );
    // Browser time with no site is one row with an empty domain ("Other"),
    // which never reaches `sites`.
    assert_eq!(
        sum.apps[0].domains,
        domains(&[("github.com", 10_000), ("youtube.com", 10_000), ("", 5_000)])
    );
    assert_eq!(
        sum.sites,
        domains(&[("github.com", 10_000), ("youtube.com", 10_000)])
    );
}

#[test]
fn hints_only_apply_to_the_browser_they_name() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Macos);
    m.web_hint(0, "chrome".into(), page("https://news.ycombinator.com/"))
        .unwrap();
    for at in [1_000, 6_000] {
        m.heartbeat(at, Some(app("org.mozilla.firefox", "Firefox")))
            .unwrap();
    }
    for at in [6_000, 11_000] {
        m.heartbeat(at, Some(app("com.apple.Terminal", "Terminal")))
            .unwrap();
    }
    for at in [11_000, 16_000] {
        m.heartbeat(at, Some(app("com.google.Chrome", "Google Chrome")))
            .unwrap();
    }
    // An activity that carries its own URL (macOS AppleScript) ignores hints.
    for at in [16_000, 21_000] {
        m.heartbeat(
            at,
            Some(browsing("com.google.Chrome", "https://docs.rs/serde")),
        )
        .unwrap();
    }
    m.flush(21_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(1_000, 6_000, "org.mozilla.firefox", None),
            s(6_000, 11_000, "com.apple.Terminal", None),
            s(
                11_000,
                16_000,
                "com.google.Chrome",
                Some("news.ycombinator.com")
            ),
            s(16_000, 21_000, "com.google.Chrome", Some("docs.rs")),
        ]
    );
}

#[test]
fn hints_from_unknown_browsers_apply_to_any_browser() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    m.web_hint(0, "SomeNewBrowser".into(), page("https://example.org/"))
        .unwrap();
    m.heartbeat(1_000, Some(app("brave-browser", "Brave")))
        .unwrap();
    m.heartbeat(2_000, Some(app("brave-browser", "Brave")))
        .unwrap();
    assert_eq!(
        spans(&m),
        vec![s(1_000, 2_000, "brave-browser", Some("example.org"))]
    );
    m.flush(2_000).unwrap();

    // The extension's fallback in a Chromium fork it cannot identify.
    m.web_hint(
        100_000,
        "chromium-family".into(),
        page("https://fork.example/"),
    )
    .unwrap();
    m.heartbeat(101_000, Some(app("brave-browser", "Brave")))
        .unwrap();
    m.heartbeat(106_000, Some(app("company.thebrowser.Browser", "Arc")))
        .unwrap();
    m.heartbeat(111_000, Some(app("company.thebrowser.Browser", "Arc")))
        .unwrap();
    m.flush(111_000).unwrap();
    // `chromium` names Chromium itself, so it never labels Brave.
    m.web_hint(
        200_000,
        "chromium".into(),
        page("https://chromium.example/"),
    )
    .unwrap();
    m.heartbeat(201_000, Some(app("brave-browser", "Brave")))
        .unwrap();
    m.heartbeat(206_000, Some(app("brave-browser", "Brave")))
        .unwrap();
    assert_eq!(
        spans(&m)[1..],
        [
            s(101_000, 106_000, "brave-browser", Some("fork.example")),
            s(
                106_000,
                111_000,
                "company.thebrowser.Browser",
                Some("fork.example")
            ),
            s(201_000, 206_000, "brave-browser", None),
        ]
    );
}

#[test]
fn a_no_page_hint_ends_the_previous_sites_time() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    let chrome = app("google-chrome", "Chrome");
    m.web_hint(0, "chrome".into(), page("https://youtube.com/"))
        .unwrap();
    m.heartbeat(1_000, Some(chrome.clone())).unwrap();
    // A new tab page: the extension reports no page.
    m.web_cleared(4_000, "chrome".into()).unwrap();
    m.heartbeat(6_000, Some(chrome.clone())).unwrap();
    m.heartbeat(11_000, Some(chrome.clone())).unwrap();
    // A fork the extension cannot identify clears whichever browser is focused.
    m.web_hint(12_000, "chrome".into(), page("https://github.com/"))
        .unwrap();
    m.heartbeat(16_000, Some(chrome.clone())).unwrap();
    m.web_cleared(17_000, "chromium-family".into()).unwrap();
    m.heartbeat(21_000, Some(chrome.clone())).unwrap();
    m.heartbeat(26_000, Some(chrome)).unwrap();
    m.flush(26_000).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(1_000, 6_000, "google-chrome", Some("youtube.com")),
            s(6_000, 16_000, "google-chrome", None),
            s(16_000, 21_000, "google-chrome", Some("github.com")),
            s(21_000, 26_000, "google-chrome", None),
        ]
    );
    let sum = m.summary(query(0, 100_000, 0)).unwrap();
    assert_eq!(
        sum.apps[0].domains,
        domains(&[("", 15_000), ("github.com", 5_000), ("youtube.com", 5_000)])
    );
}

#[test]
fn a_no_page_hint_splits_intervals_and_blanks_the_first_piece() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Android);
    let chrome = app("com.android.chrome", "Chrome");
    m.web_hint(0, "com.android.chrome".into(), page("youtube.com/"))
        .unwrap();
    m.web_cleared(30_000, "com.android.chrome".into()).unwrap();
    m.web_hint(60_000, "com.android.chrome".into(), page("github.com/"))
        .unwrap();
    m.web_cleared(150_000, "com.android.chrome".into()).unwrap();
    m.record_interval(10_000, 100_000, chrome.clone()).unwrap();
    m.record_interval(200_000, 210_000, chrome).unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(10_000, 30_000, "com.android.chrome", Some("youtube.com")),
            s(30_000, 60_000, "com.android.chrome", None),
            s(60_000, 100_000, "com.android.chrome", Some("github.com")),
            s(200_000, 210_000, "com.android.chrome", None),
        ]
    );
}

#[test]
fn intervals_only_take_recent_hints_from_their_own_browser() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Android);
    let day = 86_400_000;
    // Kiwi is not in the browser table.
    m.web_hint(
        1_000,
        "com.kiwibrowser.browser".into(),
        page("https://secret.example/"),
    )
    .unwrap();
    m.record_interval(1_000, 2_000, app("com.kiwibrowser.browser", "Kiwi"))
        .unwrap();
    m.web_hint(
        5_000,
        "com.kiwibrowser.browser".into(),
        page("https://inside.example/"),
    )
    .unwrap();
    m.record_interval(4_000, 6_000, app("com.android.chrome", "Chrome"))
        .unwrap();
    // A Chrome hint three days before the next Chrome interval is stale.
    m.web_hint(
        10_000,
        "com.android.chrome".into(),
        page("https://old.example/"),
    )
    .unwrap();
    m.record_interval(
        3 * day,
        3 * day + 600_000,
        app("com.android.chrome", "Chrome"),
    )
    .unwrap();
    // One 29 minutes before still names the page in front.
    m.web_hint(
        4 * day - 29 * 60_000,
        "com.android.chrome".into(),
        page("https://fresh.example/"),
    )
    .unwrap();
    m.record_interval(
        4 * day,
        4 * day + 1_000,
        app("com.android.chrome", "Chrome"),
    )
    .unwrap();
    assert_eq!(
        spans(&m),
        vec![
            s(1_000, 2_000, "com.kiwibrowser.browser", None),
            s(4_000, 6_000, "com.android.chrome", None),
            s(3 * day, 3 * day + 600_000, "com.android.chrome", None),
            s(
                4 * day,
                4 * day + 1_000,
                "com.android.chrome",
                Some("fresh.example")
            ),
        ]
    );
}

#[test]
fn intervals_split_at_hints_and_reingest_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Android);
    let chrome = app("com.android.chrome", "Chrome");
    m.web_hint(
        -5_000,
        "com.android.chrome".into(),
        page("news.ycombinator.com/item?id=1"),
    )
    .unwrap();
    m.web_hint(
        20_000,
        "com.android.chrome".into(),
        page("m.youtube.com/watch?v=2"),
    )
    .unwrap();
    m.web_hint(60_000, "com.android.chrome".into(), page("github.com/"))
        .unwrap();
    m.web_hint(500_000, "com.android.chrome".into(), page("later.example/"))
        .unwrap();

    m.record_interval(0, 100_000, chrome.clone()).unwrap();
    let first = m.recent_spans(100).unwrap();
    m.record_interval(0, 100_000, chrome.clone()).unwrap();
    assert_eq!(
        m.recent_spans(100).unwrap(),
        first,
        "re-ingesting changes nothing, rev included"
    );
    assert_eq!(
        spans(&m),
        vec![
            s(
                0,
                20_000,
                "com.android.chrome",
                Some("news.ycombinator.com")
            ),
            s(20_000, 60_000, "com.android.chrome", Some("youtube.com")),
            s(60_000, 100_000, "com.android.chrome", Some("github.com")),
        ]
    );

    // UsageStats reports the same foreground interval again, now longer.
    m.record_interval(0, 120_000, chrome.clone()).unwrap();
    let last = &m.recent_spans(1).unwrap()[0];
    assert_eq!((last.start_ms, last.end_ms), (60_000, 120_000));
    assert!(last.rev > first[0].rev);

    let maps = app("com.google.android.apps.maps", "Maps");
    m.record_interval(200_000, 260_000, maps.clone()).unwrap();
    m.record_interval(200_000, 260_000, maps.clone()).unwrap();
    m.record_interval(300_000, 300_000, maps).unwrap();

    let sum = m.summary(query(0, 400_000, 400_000)).unwrap();
    assert_eq!(sum.total_ms, 180_000);
    let apps: Vec<_> = sum
        .apps
        .iter()
        .map(|a| (a.app_key.as_str(), a.total_ms))
        .collect();
    assert_eq!(
        apps,
        vec![
            ("chrome", 120_000),
            ("com.google.android.apps.maps", 60_000)
        ]
    );
    assert_eq!(
        sum.apps[0].domains,
        domains(&[
            ("github.com", 60_000),
            ("youtube.com", 40_000),
            ("news.ycombinator.com", 20_000)
        ])
    );
    assert_eq!(sum.apps[1].domains, vec![]);
}

#[test]
fn hints_that_repeat_the_same_url_do_not_split_an_interval() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Android);
    for i in 1..=10 {
        m.web_hint(
            i * 10_000,
            "com.android.chrome".into(),
            page("en.wikipedia.org/wiki/Rust"),
        )
        .unwrap();
    }
    m.record_interval(10_000, 200_000, app("com.android.chrome", "Chrome"))
        .unwrap();
    assert_eq!(
        spans(&m),
        vec![s(
            10_000,
            200_000,
            "com.android.chrome",
            Some("en.wikipedia.org")
        )]
    );

    // An hour later no hint is fresh, and a no-page hint inside the interval
    // changes nothing about it either: one piece with no site.
    m.web_cleared(3_700_000, "com.android.chrome".into())
        .unwrap();
    m.record_interval(3_600_000, 3_800_000, app("com.android.chrome", "Chrome"))
        .unwrap();
    assert_eq!(
        spans(&m)[1..],
        [s(3_600_000, 3_800_000, "com.android.chrome", None)]
    );
}

#[test]
fn domains_are_normalized() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Macos);
    let urls = [
        "https://www.YouTube.com/watch?v=1",
        "http://m.youtube.com:8080/feed",
        "https://mobile.twitter.com/home",
        "https://twitter.com/",
        "https://en.m.wikipedia.org/wiki/Rust",
        "chrome://settings",
    ];
    for (i, url) in urls.iter().enumerate() {
        let start = i as i64 * 10_000;
        m.record_interval(start, start + 10_000, browsing("com.google.Chrome", url))
            .unwrap();
    }
    let sum = m.summary(query(0, 100_000, 100_000)).unwrap();
    assert_eq!(
        sum.sites,
        domains(&[
            ("twitter.com", 20_000),
            ("youtube.com", 20_000),
            ("en.m.wikipedia.org", 10_000)
        ])
    );
    assert_eq!(sum.apps[0].total_ms, 60_000);
}

#[test]
fn summary_clips_to_range_and_fills_buckets() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Android);
    m.record_interval(0, 10_000, app("a", "A")).unwrap();
    m.record_interval(1_800_000, 1_810_000, app("b", "B"))
        .unwrap();
    m.record_interval(3_598_000, 3_603_000, app("a", "A"))
        .unwrap();
    m.record_interval(4_000_000, 4_100_000, app("c", "C"))
        .unwrap();

    let sum = m.summary(query(5_000, 3_601_000, 1_800_000)).unwrap();
    assert_eq!(
        (sum.start_ms, sum.end_ms, sum.total_ms),
        (5_000, 3_601_000, 18_000)
    );
    assert_eq!(sum.buckets, vec![10_000, 8_000]);
    let apps: Vec<_> = sum
        .apps
        .iter()
        .map(|a| (a.app_key.as_str(), a.total_ms))
        .collect();
    assert_eq!(apps, vec![("b", 10_000), ("a", 8_000)]);

    let own = m.device().id;
    let mine = m
        .summary(SummaryQuery {
            device_id: Some(own),
            ..query(5_000, 3_601_000, 1_800_000)
        })
        .unwrap();
    assert_eq!(mine.total_ms, 18_000);
    let other = m
        .summary(SummaryQuery {
            device_id: Some("someone-else".into()),
            ..query(0, 5_000_000, 0)
        })
        .unwrap();
    assert_eq!(
        (other.total_ms, other.by_device.len(), other.buckets.len()),
        (0, 0, 0)
    );
}

#[test]
fn summary_survives_extreme_ranges_and_caps_buckets() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Android);
    m.record_interval(0, 10_000, app("a", "A")).unwrap();

    let all_time = m.summary(query(i64::MIN, i64::MAX, 0)).unwrap();
    assert_eq!((all_time.total_ms, all_time.buckets.len()), (10_000, 0));
    let wide = m.summary(query(i64::MIN, i64::MAX, i64::MAX)).unwrap();
    assert_eq!(wide.buckets, vec![0, 10_000, 0]);

    let most = m.summary(query(0, 10_000_000, 1_000)).unwrap();
    assert_eq!((most.buckets.len(), most.buckets[0]), (10_000, 1_000));
    let too_many = m.summary(query(0, 10_000_001, 1_000));
    assert!(
        matches!(too_many, Err(SpentDayError::InvalidArgument(_))),
        "{too_many:?}"
    );
    let huge = m.summary(query(i64::MIN, i64::MAX, 1));
    assert!(
        matches!(huge, Err(SpentDayError::InvalidArgument(_))),
        "{huge:?}"
    );

    // Two all-of-time spans overflow every bucket, which saturates instead.
    m.record_interval(i64::MIN, i64::MAX, app("b", "B"))
        .unwrap();
    m.record_interval(i64::MIN, i64::MAX, app("c", "C"))
        .unwrap();
    let saturated = m.summary(query(i64::MIN, i64::MAX, i64::MAX)).unwrap();
    assert_eq!(
        (saturated.total_ms, saturated.buckets),
        (i64::MAX, vec![i64::MAX, i64::MAX, 2])
    );

    let backwards = m.summary(query(10_000, 0, 1_000)).unwrap();
    assert_eq!(
        (
            backwards.total_ms,
            backwards.apps.len(),
            backwards.buckets.len()
        ),
        (0, 0, 0)
    );
}

#[test]
fn same_browser_groups_across_platform_ids() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    m.record_interval(
        0,
        10_000,
        browsing("com.google.Chrome", "https://a.example/"),
    )
    .unwrap();
    m.record_interval(
        10_000,
        30_000,
        browsing("Google-chrome", "https://a.example/"),
    )
    .unwrap();
    m.record_interval(
        30_000,
        60_000,
        browsing("com.android.chrome", "https://b.example/"),
    )
    .unwrap();
    m.record_interval(60_000, 65_000, app("com.example.Thing", "Thing"))
        .unwrap();
    let sum = m.summary(query(0, 100_000, 100_000)).unwrap();
    let apps: Vec<_> = sum
        .apps
        .iter()
        .map(|a| {
            (
                a.app_key.as_str(),
                a.app_name.as_str(),
                a.is_browser,
                a.total_ms,
            )
        })
        .collect();
    assert_eq!(
        apps,
        vec![
            ("chrome", "Chrome", true, 60_000),
            ("com.example.Thing", "Thing", false, 5_000)
        ]
    );
    assert_eq!(
        sum.apps[0].domains,
        domains(&[("a.example", 30_000), ("b.example", 30_000)])
    );
    let app_ids: Vec<_> = sum.apps.iter().map(|a| a.app_ids.clone()).collect();
    assert_eq!(
        app_ids,
        vec![
            vec![
                "com.android.chrome".to_owned(),
                "Google-chrome".to_owned(),
                "com.google.Chrome".to_owned()
            ],
            vec!["com.example.Thing".to_owned()]
        ]
    );
}

#[test]
fn duckduckgo_is_a_browser_on_android_and_macos() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Android);
    m.web_hint(
        0,
        "com.duckduckgo.mobile.android".into(),
        page("https://duckduckgo.com/?q=rust"),
    )
    .unwrap();
    m.record_interval(
        1_000,
        11_000,
        app("com.duckduckgo.mobile.android", "DuckDuckGo"),
    )
    .unwrap();
    m.record_interval(
        11_000,
        16_000,
        browsing("com.duckduckgo.macos.browser", "https://docs.rs/"),
    )
    .unwrap();
    let sum = m.summary(query(0, 100_000, 0)).unwrap();
    let apps: Vec<_> = sum
        .apps
        .iter()
        .map(|a| (a.app_key.as_str(), a.app_name.as_str(), a.is_browser))
        .collect();
    assert_eq!(apps, vec![("duckduckgo", "DuckDuckGo", true)]);
    assert_eq!(
        sum.apps[0].domains,
        domains(&[("duckduckgo.com", 10_000), ("docs.rs", 5_000)])
    );
}

#[test]
fn every_single_symbol_typo_in_a_sync_code_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    let code = "0W3G-E1R7-0W3G-E1R7-0W3G-E1R7-0W3G-E1R7-0W3G-E1R7-0W3G-E1R7-0W3M-QC3F";
    let config = |sync_code: String| {
        Some(spentday_core::SyncConfig {
            server_url: "http://127.0.0.1:1".into(),
            sync_code,
        })
    };
    m.set_sync_config(config(code.into())).unwrap();
    let symbols = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut accepted = Vec::new();
    for (i, typed) in code.char_indices().filter(|&(_, c)| c != '-') {
        for typo in symbols.chars().filter(|&c| c != typed) {
            let mistyped = format!("{}{typo}{}", &code[..i], &code[i + 1..]);
            if m.set_sync_config(config(mistyped.clone())).is_ok() {
                accepted.push(mistyped);
            }
        }
    }
    assert_eq!(accepted, Vec::<String>::new());
    assert_eq!(m.sync_config().unwrap(), config(code.into()));
}

#[test]
fn sync_needs_a_valid_code() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    assert!(matches!(m.sync_now(), Err(SpentDayError::SyncNotConfigured)));
    let bad = spentday_core::SyncConfig {
        server_url: "http://127.0.0.1:1".into(),
        sync_code: "not-a-code".into(),
    };
    assert!(matches!(
        m.set_sync_config(Some(bad)),
        Err(SpentDayError::InvalidSyncCode)
    ));
    assert_eq!(m.sync_config().unwrap(), None);

    let code = m.new_sync_code();
    assert_eq!((code.len(), code.split('-').count()), (69, 14));
    let typed = code.to_lowercase().replace('-', " ");
    m.set_sync_config(Some(spentday_core::SyncConfig {
        server_url: "http://127.0.0.1:1/".into(),
        sync_code: typed,
    }))
    .unwrap();
    assert_eq!(
        m.sync_config().unwrap(),
        Some(spentday_core::SyncConfig {
            server_url: "http://127.0.0.1:1".into(),
            sync_code: code
        })
    );
    assert!(matches!(m.sync_now(), Err(SpentDayError::Network(_))));
    m.set_sync_config(None).unwrap();
    assert_eq!(m.sync_config().unwrap(), None);
}

#[test]
fn hints_survive_reopen_until_a_week_older_than_the_newest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spentday.db").to_string_lossy().into_owned();
    let week = 7 * 24 * 3_600_000;
    let first = open(&dir, Platform::Android);
    first
        .web_hint(1_000, "com.android.chrome".into(), page("github.com/"))
        .unwrap();
    drop(first);

    let second = SpentDay::open(path.clone(), "phone".into(), Platform::Android).unwrap();
    second
        .record_interval(1_000, 2_000, app("com.android.chrome", "Chrome"))
        .unwrap();
    second
        .web_hint(
            1_000 + week + 1,
            "com.android.chrome".into(),
            page("example.com/"),
        )
        .unwrap();
    drop(second);

    let third = SpentDay::open(path, "phone".into(), Platform::Android).unwrap();
    third
        .record_interval(3_000, 4_000, app("com.android.chrome", "Chrome"))
        .unwrap();
    assert_eq!(
        spans(&third),
        vec![
            s(1_000, 2_000, "com.android.chrome", Some("github.com")),
            s(3_000, 4_000, "com.android.chrome", None),
        ]
    );
}

fn wall_clock_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn future_hints_are_dropped_and_never_prune_real_ones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spentday.db").to_string_lossy().into_owned();
    let chrome = || app("com.android.chrome", "Chrome");
    let m = open(&dir, Platform::Android);
    // A phone with its clock set to 2096.
    m.web_hint(
        4_000_000_000_000,
        "com.android.chrome".into(),
        page("https://future.example/"),
    )
    .unwrap();
    m.web_hint(
        1_000,
        "com.android.chrome".into(),
        page("https://real.example/"),
    )
    .unwrap();
    drop(m);
    let m = SpentDay::open(path, "phone".into(), Platform::Android).unwrap();
    m.record_interval(2_000, 3_000, chrome()).unwrap();
    assert_eq!(
        spans(&m),
        vec![s(2_000, 3_000, "com.android.chrome", Some("real.example"))]
    );

    // A hint up to a day ahead is kept, but retention counts back from now.
    let now = wall_clock_ms();
    let week = 7 * 24 * 3_600_000;
    m.web_hint(
        now - week + 3_600_000,
        "com.android.chrome".into(),
        page("https://week.example/"),
    )
    .unwrap();
    m.web_hint(
        now + 23 * 3_600_000,
        "com.android.chrome".into(),
        page("https://soon.example/"),
    )
    .unwrap();
    m.record_interval(now - week + 3_601_000, now - week + 3_602_000, chrome())
        .unwrap();
    assert_eq!(
        spans(&m)[1],
        s(
            now - week + 3_601_000,
            now - week + 3_602_000,
            "com.android.chrome",
            Some("week.example")
        )
    );
}

#[test]
fn hints_are_pruned_while_the_app_runs() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Android);
    let week = 7 * 24 * 3_600_000;
    m.web_hint(
        1_000,
        "com.android.chrome".into(),
        page("https://old.example/"),
    )
    .unwrap();
    m.web_hint(
        1_000 + week + 1,
        "com.android.chrome".into(),
        page("https://new.example/"),
    )
    .unwrap();
    m.record_interval(2_000, 3_000, app("com.android.chrome", "Chrome"))
        .unwrap();
    assert_eq!(spans(&m), vec![s(2_000, 3_000, "com.android.chrome", None)]);
}

#[test]
fn long_urls_keep_their_domain_and_app_names_are_clipped_on_a_char_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let m = open(&dir, Platform::Linux);
    m.web_hint(
        1_000,
        "firefox".into(),
        page(&format!("https://example.com/{}", "a".repeat(4_000))),
    )
    .unwrap();
    m.heartbeat(2_000, Some(app("org.mozilla.firefox", "Firefox")))
        .unwrap();
    // No real host is longer than DNS allows, so it is no site.
    let long_host = format!("https://{}.com/", "a".repeat(300));
    m.heartbeat(3_000, Some(browsing("com.google.Chrome", &long_host)))
        .unwrap();
    m.heartbeat(
        4_000,
        Some(app("org.gnome.Terminal", &format!("x{}", "é".repeat(400)))),
    )
    .unwrap();
    let rows: Vec<_> = m
        .recent_spans(3)
        .unwrap()
        .into_iter()
        .map(|s| (s.app_name.len(), s.domain))
        .collect();
    assert_eq!(
        rows,
        vec![
            (511, None),
            ("Browser".len(), None),
            ("Firefox".len(), Some("example.com".into()))
        ]
    );
}

#[test]
fn flush_with_nothing_open_never_waits_for_the_writer_lock() {
    let dir = tempfile::tempdir().unwrap();
    let spentday = open(&dir, Platform::Macos);
    let other = rusqlite::Connection::open(dir.path().join("spentday.db")).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();

    let started = std::time::Instant::now();
    spentday.flush(10_000).unwrap();
    spentday.heartbeat(15_000, None).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(1));

    other.execute_batch("ROLLBACK").unwrap();
}
