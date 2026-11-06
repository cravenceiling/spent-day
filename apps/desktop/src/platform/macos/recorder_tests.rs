use super::{browser::BrowserTab, front::FrontApp, recorder::Recorder, test_support::*};
fn app(id: &str, name: &str, pid: i32) -> Option<FrontApp> {
    Some(FrontApp {
        id: id.into(),
        name: name.into(),
        pid,
    })
}
fn chrome() -> Option<FrontApp> {
    app("com.google.Chrome", "Google Chrome", 200)
}
fn finder() -> Option<FrontApp> {
    app("com.apple.finder", "Finder", 100)
}
fn safari() -> Option<FrontApp> {
    app("com.apple.Safari", "Safari", 300)
}

#[test]
fn first_asynchronous_answer() {
    let dir = Scratch::new();
    let core = dir.core();
    let (fake, pages) = Fake::new();
    let mut recorder = Recorder::new(core.clone(), pages);
    fake.show(page("https://youtube.com/watch"));
    recorder.record(finder(), t0(), false).unwrap();
    recorder.record(chrome(), t0() + 5_000, false).unwrap();
    fake.wait();
    recorder.record(chrome(), t0() + 6_000, false).unwrap();
    core.flush(t0() + 8_000).unwrap();
    assert_eq!(sites(&core, "com.apple.finder"), expected(&[("", 5_000)]));
    assert_eq!(
        sites(&core, "com.google.Chrome"),
        expected(&[("youtube.com", 2_000), ("", 1_000)])
    );
}
#[test]
fn reactivation_and_window_change_invalidate_old_pages() {
    for reactivation in [true, false] {
        let dir = Scratch::new();
        let core = dir.core();
        let (fake, pages) = Fake::new();
        let mut recorder = Recorder::new(core.clone(), pages);
        fake.show(page("https://a.example/"));
        recorder.activated();
        recorder.record(chrome(), t0(), false).unwrap();
        fake.wait();
        recorder.record(chrome(), t0() + 1_000, false).unwrap();
        if reactivation {
            recorder.activated();
            recorder.record(finder(), t0() + 2_000, false).unwrap();
            recorder.activated();
        } else {
            fake.window(Some(2));
        }
        fake.hold();
        fake.show(page("https://b.example/"));
        let changed = if reactivation { 3_000 } else { 2_000 };
        recorder.record(chrome(), t0() + changed, false).unwrap();
        fake.started();
        fake.started();
        fake.release();
        fake.wait();
        recorder
            .record(chrome(), t0() + changed + 1_000, false)
            .unwrap();
        core.flush(t0() + changed + 2_000).unwrap();
        assert_eq!(
            sites(&core, "com.google.Chrome"),
            expected(&[("a.example", 1_000), ("b.example", 1_000), ("", 2_000)])
        );
    }
}
#[test]
fn held_answer_spanning_a_window_change_is_dropped() {
    let dir = Scratch::new();
    let core = dir.core();
    let (fake, pages) = Fake::new();
    let mut recorder = Recorder::new(core.clone(), pages);
    fake.show(page("https://a.example/"));
    fake.hold();
    recorder.activated();
    recorder.record(chrome(), t0(), false).unwrap();
    fake.started();
    fake.window(Some(2));
    fake.release();
    fake.wait();
    recorder.record(chrome(), t0() + 1_000, false).unwrap();
    core.flush(t0() + 2_000).unwrap();
    assert_eq!(sites(&core, "com.google.Chrome"), expected(&[("", 2_000)]));
}
#[test]
fn safari_is_queried_only_after_opt_in() {
    for enabled in [false, true] {
        let dir = Scratch::new();
        let core = dir.core();
        let (fake, pages) = Fake::new();
        let mut recorder = Recorder::new(core.clone(), pages);
        fake.show(page("https://apple.com/"));
        recorder.activated();
        recorder.record(safari(), t0(), enabled).unwrap();
        if enabled {
            fake.wait();
        } else {
            assert_eq!(fake.asked(), 0);
        }
        recorder.record(safari(), t0() + 1_000, enabled).unwrap();
        core.flush(t0() + 2_000).unwrap();
        assert_eq!(
            sites(&core, "com.apple.Safari"),
            if enabled {
                expected(&[("", 1_000), ("apple.com", 1_000)])
            } else {
                expected(&[("", 2_000)])
            }
        );
    }
}
#[test]
fn incognito_stops_the_previous_page() {
    let dir = Scratch::new();
    let core = dir.core();
    let (fake, pages) = Fake::new();
    let mut recorder = Recorder::new(core.clone(), pages);
    fake.show(page("https://a.example/watch"));
    recorder.activated();
    recorder.record(chrome(), t0(), false).unwrap();
    fake.wait();
    recorder.record(chrome(), t0() + 1_000, false).unwrap();
    fake.window(Some(2));
    fake.show(BrowserTab::NoPage);
    recorder.record(chrome(), t0() + 2_000, false).unwrap();
    fake.wait();
    recorder.record(chrome(), t0() + 3_000, false).unwrap();
    recorder.record(chrome(), t0() + 4_000, false).unwrap();
    core.flush(t0() + 5_000).unwrap();
    assert_eq!(
        sites(&core, "com.google.Chrome"),
        expected(&[("a.example", 1_000), ("", 4_000)])
    );
}
#[test]
fn away_rolls_back_closed_page_spans() {
    let dir = Scratch::new();
    let core = dir.core();
    for at in (0..=185_000).step_by(5_000) {
        let url = if at < 120_000 {
            "https://a.example/"
        } else if at < 175_000 {
            "https://b.example/"
        } else {
            "https://c.example/"
        };
        core.heartbeat(
            t0() + at,
            Some(spentday_core::Activity {
                app_id: "com.google.Chrome".into(),
                app_name: "Chrome".into(),
                web: Some(spentday_core::WebPage { url: url.into() }),
            }),
        )
        .unwrap();
    }
    core.away_since(t0(), t0() + 185_000).unwrap();
    assert_eq!(summary(&core).total_ms, 0);
}

#[test]
fn no_page_clears_only_on_transition_and_unknown_keeps_extension_hints() {
    let dir = Scratch::new();
    let core = dir.core();
    let (fake, pages) = Fake::new();
    let mut recorder = Recorder::new(core.clone(), pages);
    fake.show(BrowserTab::NoPage);
    core.web_hint(
        t0(),
        "chrome".into(),
        spentday_core::WebPage {
            url: "https://a.example/".into(),
        },
    )
    .unwrap();
    recorder.record(chrome(), t0(), false).unwrap();
    fake.wait();
    recorder.record(chrome(), t0() + 1_000, false).unwrap();
    core.web_hint(
        t0() + 2_000,
        "chrome".into(),
        spentday_core::WebPage {
            url: "https://b.example/".into(),
        },
    )
    .unwrap();
    recorder.record(chrome(), t0() + 2_000, false).unwrap();
    fake.error(-1743);
    fake.window(Some(2));
    recorder.record(chrome(), t0() + 3_000, false).unwrap();
    fake.wait();
    recorder.record(chrome(), t0() + 4_000, false).unwrap();
    core.flush(t0() + 5_000).unwrap();
    assert_eq!(
        sites(&core, "com.google.Chrome"),
        expected(&[("a.example", 1_000), ("b.example", 3_000), ("", 1_000)])
    );
}
