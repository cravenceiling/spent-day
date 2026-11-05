use spentday_core::{DayQuery, ItemKind, Platform, SpentDay};

fn read(
    rows: &[(&str, i64, i64, &str, Option<&str>)],
    start: i64,
    end: i64,
) -> spentday_core::DayDetail {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("day.db");
    let core = SpentDay::open(
        path.to_string_lossy().into(),
        "Test".into(),
        Platform::Macos,
    )
    .unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    for (n, &(device, from, to, app, domain)) in rows.iter().enumerate() {
        db.execute(
            "INSERT OR IGNORE INTO devices(id,name,platform) VALUES (?1,?1,'macos')",
            [device],
        )
        .unwrap();
        db.execute("INSERT INTO spans(device_id,id,start_ms,end_ms,app_id,app_name,domain,rev) VALUES (?1,?2,?3,?4,?5,?5,?6,1)",
            rusqlite::params![device,n.to_string(),from,to,app,domain]).unwrap();
    }
    core.day(DayQuery {
        start_ms: start,
        end_ms: end,
    })
    .unwrap()
}

#[test]
fn union_and_separate_device_totals() {
    let d = read(
        &[
            ("a", 0, 120_000, "a", None),
            ("b", 60_000, 180_000, "b", None),
        ],
        0,
        200_000,
    );
    assert_eq!(d.wall_ms, 180_000);
    assert_eq!(d.device_ms, 240_000);
    assert_eq!(
        d.devices.iter().map(|d| d.total_ms).collect::<Vec<_>>(),
        [120_000, 120_000]
    );
}

#[test]
fn merges_at_sixty_seconds_but_not_sixty_one() {
    let d = read(
        &[
            ("a", 0, 10_000, "a", None),
            ("a", 70_000, 80_000, "a", None),
            ("a", 141_000, 150_000, "a", None),
        ],
        0,
        200_000,
    );
    let i = &d.devices[0].items[0];
    assert_eq!(i.total_ms, 89_000);
    assert_eq!(
        i.sessions
            .iter()
            .map(|s| (s.start_ms, s.end_ms))
            .collect::<Vec<_>>(),
        [(0, 80_000), (141_000, 150_000)]
    );
}

#[test]
fn intervening_item_prevents_merging() {
    let d = read(
        &[
            ("a", 0, 10_000, "a", None),
            ("a", 10_000, 20_000, "b", None),
            ("a", 20_000, 30_000, "a", None),
        ],
        0,
        40_000,
    );
    assert_eq!(d.devices[0].items[0].sessions.len(), 2);
    assert_eq!(d.wall_ms, 30_000);
}

#[test]
fn site_split_and_cross_browser_merge() {
    let d = read(
        &[
            ("a", 0, 10_000, "com.google.Chrome", Some("example.org")),
            (
                "a",
                10_000,
                20_000,
                "org.mozilla.firefox",
                Some("example.org"),
            ),
            ("a", 20_000, 25_000, "com.google.Chrome", None),
        ],
        0,
        30_000,
    );
    let items = &d.devices[0].items;
    assert_eq!(items.len(), 2);
    assert_eq!(
        (&*items[0].key, items[0].kind, items[0].total_ms),
        ("site:example.org", ItemKind::Site, 20_000)
    );
    assert_eq!(
        items[0].app_ids,
        ["com.google.Chrome", "org.mozilla.firefox"]
    );
    assert_eq!(
        (
            &*items[1].key,
            &*items[1].name,
            &*items[1].app_key,
            items[1].total_ms
        ),
        ("app:chrome", "Chrome", "chrome", 5_000)
    );
    assert_eq!(d.wall_ms, 25_000);
}

#[test]
fn clips_midnight_and_skips_zero_length() {
    let d = read(
        &[
            ("a", 86_390_000, 86_410_000, "a", None),
            ("a", 172_790_000, 172_810_000, "b", None),
            ("a", 100_000_000, 100_000_000, "c", None),
        ],
        86_400_000,
        172_800_000,
    );
    assert_eq!(d.wall_ms, 20_000);
    assert_eq!(d.device_ms, 20_000);
    assert_eq!(d.devices[0].items.len(), 2);
    assert_eq!(d.devices[0].items[0].sessions[0].start_ms, 86_400_000);
    assert_eq!(d.devices[0].items[1].sessions[0].end_ms, 172_800_000);
}

#[test]
fn canonical_app_and_ranked_platform_ids_match_summary() {
    let d = read(
        &[
            ("a", 0, 10_000, "com.google.Chrome", None),
            ("a", 10_000, 30_000, "com.android.chrome", None),
        ],
        0,
        40_000,
    );
    let item = &d.devices[0].items[0];
    assert_eq!(item.key, "app:chrome");
    assert_eq!(item.name, "Chrome");
    assert_eq!(item.app_ids, ["com.android.chrome", "com.google.Chrome"]);
    assert_eq!(item.total_ms, 30_000);
    assert_eq!(item.sessions.len(), 1);
}

#[test]
fn app_name_comes_from_span_with_latest_end_even_when_clipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("day.db");
    let core = SpentDay::open(
        path.to_string_lossy().into(),
        "Test".into(),
        Platform::Macos,
    )
    .unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute(
        "INSERT INTO devices(id,name,platform) VALUES ('a','Test','macos')",
        [],
    )
    .unwrap();
    for (id, start, end, name) in [
        ("long", 0, 300_000, "Latest name"),
        ("nested", 100_000, 200_000, "Older name"),
    ] {
        db.execute("INSERT INTO spans(device_id,id,start_ms,end_ms,app_id,app_name,rev) VALUES ('a',?1,?2,?3,'example.app',?4,1)",
            rusqlite::params![id, start, end, name]).unwrap();
    }
    let detail = core
        .day(DayQuery {
            start_ms: 0,
            end_ms: 150_000,
        })
        .unwrap();
    assert_eq!(detail.devices[0].items[0].name, "Latest name");
}
