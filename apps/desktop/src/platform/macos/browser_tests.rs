use super::super::test_support::{Fake, page};
use super::*;
#[test]
fn all_eleven_scripts_are_byte_exact() {
    let chromium = "set w to window id (id of front window)\nif mode of w is \"incognito\" then return {}\nreturn {URL of active tab of w}";
    let safari = "set w to window id (id of front window)\nreturn {URL of current tab of w}";
    let arc = "set w to window id (id of front window)\nif incognito of w then return {}\nreturn {URL of active tab of w}";
    let mut expected = [
        ("com.apple.Safari", safari),
        ("com.apple.SafariTechnologyPreview", safari),
        ("com.google.Chrome", chromium),
        ("com.google.Chrome.beta", chromium),
        ("com.google.Chrome.canary", chromium),
        ("com.brave.Browser", chromium),
        ("net.imput.helium", chromium),
        ("com.microsoft.edgemac", chromium),
        ("com.vivaldi.Vivaldi", chromium),
        ("com.operasoftware.Opera", chromium),
        ("company.thebrowser.Browser", arc),
    ];
    expected.sort_by_key(|(id, _)| *id);
    assert_eq!(BUNDLE_IDS, expected.map(|(id, _)| id));
    for (id, body) in expected {
        let want = format!(
            "with timeout of 2 seconds\n    tell application id \"{id}\"\n        {body}\n    end tell\nend timeout"
        );
        assert_eq!(source(id).unwrap().as_bytes(), want.as_bytes());
    }
    assert_eq!(source("org.mozilla.firefox"), None);
}
#[test]
fn descriptor_literals() {
    fn answer(url: &str) -> Retained<NSAppleEventDescriptor> {
        let list = NSAppleEventDescriptor::listDescriptor();
        let string = NSAppleEventDescriptor::descriptorWithString(&NSString::from_str(url));
        list.insertDescriptor_atIndex(&string, 1);
        list
    }
    assert_eq!(
        decode(&answer("file:///Users/me/notes.html")),
        BrowserTab::NoPage
    );
    assert_eq!(
        decode(&answer("https://a.example/")),
        page("https://a.example/")
    );
    assert_eq!(
        decode(&NSAppleEventDescriptor::listDescriptor()),
        BrowserTab::NoPage
    );
    assert_eq!(decode(&answer("HTTPS://a.example/")), BrowserTab::NoPage);
}
#[test]
fn cache_refreshes_at_one_second_and_expires_at_ten() {
    let (fake, pages) = Fake::new();
    fake.show(page("https://a.example/"));
    assert_eq!(pages.tab("com.google.Chrome", 200, 0), None);
    fake.wait();
    fake.advance(999);
    assert_eq!(
        pages.tab("com.google.Chrome", 200, 0),
        Some(page("https://a.example/"))
    );
    assert_eq!(fake.asked(), 1);
    fake.hold();
    fake.advance(1);
    assert_eq!(
        pages.tab("com.google.Chrome", 200, 0),
        Some(page("https://a.example/"))
    );
    fake.started();
    fake.started();
    fake.advance(9_000);
    assert_eq!(pages.tab("com.google.Chrome", 200, 0), None);
    assert_eq!(fake.asked(), 2);
    fake.release();
    fake.wait();
}
#[test]
fn pending_is_per_bundle_and_answers_keep_the_original_identity() {
    let (fake, pages) = Fake::new();
    fake.hold();
    fake.show(page("https://a.example/"));
    assert_eq!(pages.tab("com.google.Chrome", 200, 0), None);
    fake.started();
    assert_eq!(pages.tab("com.google.Chrome", 200, 1), None);
    assert_eq!(pages.tab("com.google.Chrome", 201, 1), None);
    assert_eq!(fake.asked(), 1);
    fake.release();
    fake.wait();
    assert_eq!(pages.tab("com.google.Chrome", 201, 1), None);
    fake.wait();
    assert_eq!(
        pages.tab("com.google.Chrome", 201, 1),
        Some(page("https://a.example/"))
    );
}
#[test]
fn errors_back_off_by_bundle_but_no_window_is_explicit_no_page() {
    for (code, seconds) in [(-1743, 600), (-1708, 30)] {
        let (fake, pages) = Fake::new();
        fake.error(code);
        assert_eq!(pages.tab("com.google.Chrome", 200, 0), None);
        fake.wait();
        fake.show(page("https://a.example/"));
        fake.advance(1_000);
        fake.window(Some(2));
        assert_eq!(pages.tab("com.google.Chrome", 200, 1), None);
        fake.wait();
        assert_eq!(fake.asked(), 1);
        assert_eq!(pages.tab("com.google.Chrome", 200, 1), None);
        fake.advance((seconds - 1) * 1_000);
        assert_eq!(pages.tab("com.google.Chrome", 200, 1), None);
        fake.wait();
        assert_eq!(fake.asked(), 2);
        assert_eq!(
            pages.tab("com.google.Chrome", 200, 1),
            Some(page("https://a.example/"))
        );
    }
    let (fake, pages) = Fake::new();
    fake.error(-1728);
    pages.tab("com.google.Chrome", 200, 0);
    fake.wait();
    assert_eq!(
        pages.tab("com.google.Chrome", 200, 0),
        Some(BrowserTab::NoPage)
    );
}
#[test]
fn a_missing_window_is_a_matching_identity_and_unknown_does_not_spin() {
    let (fake, pages) = Fake::new();
    fake.window(None);
    fake.error(-1743);
    pages.tab("com.google.Chrome", 200, 0);
    fake.wait();
    for _ in 0..10 {
        assert_eq!(pages.tab("com.google.Chrome", 200, 0), None);
    }
    assert_eq!(fake.asked(), 1);
    assert_eq!(pages.tab("org.mozilla.firefox", 200, 0), None);
}

#[test]
fn pending_queries_for_two_bundles_are_independent() {
    let (fake, pages) = Fake::new();
    fake.hold();
    fake.show(page("https://a.example/"));
    pages.tab("com.google.Chrome", 200, 0);
    fake.started();
    pages.tab("com.brave.Browser", 201, 0);
    fake.release();
    fake.wait();
    fake.wait();
    assert_eq!(fake.asked(), 2);
    assert_eq!(
        pages.tab("com.google.Chrome", 200, 0),
        Some(page("https://a.example/"))
    );
    assert_eq!(
        pages.tab("com.brave.Browser", 201, 0),
        Some(page("https://a.example/"))
    );
}

fn script(source: &str) -> Retained<NSAppleScript> {
    NSAppleScript::initWithSource(NSAppleScript::alloc(), &NSString::from_str(source)).unwrap()
}

#[test]
fn applescript_errors_return_their_number_and_the_worker_keeps_answering() {
    let (answered, answers) = mpsc::channel();
    let pages = BrowserPages::spawn(
        Arc::new(|_| Some(1)),
        Arc::new(Instant::now),
        || {
            let mut scripts = Scripts::default();
            for (bundle, source) in [
                ("com.google.Chrome", "error number -1728"),
                ("com.brave.Browser", "error number -1743"),
                ("com.vivaldi.Vivaldi", "return {\"https://a.example/\"}"),
            ] {
                scripts.compiled.insert(bundle.into(), script(source));
            }
            assert_eq!(scripts.run("com.google.Chrome"), Err(-1728));
            assert_eq!(scripts.run("com.brave.Browser"), Err(-1743));
            move |id: &str| scripts.run(id)
        },
        move || answered.send(()).unwrap(),
    );
    for bundle in [
        "com.google.Chrome",
        "com.brave.Browser",
        "com.vivaldi.Vivaldi",
    ] {
        assert_eq!(pages.tab(bundle, 200, 0), None);
        answers.recv_timeout(Duration::from_secs(10)).unwrap();
    }
    assert!(pages.cache.lock().unwrap().pending.is_empty());
    assert_eq!(
        pages.tab("com.google.Chrome", 200, 0),
        Some(BrowserTab::NoPage)
    );
    assert_eq!(pages.tab("com.brave.Browser", 200, 0), None);
    assert_eq!(
        pages.tab("com.vivaldi.Vivaldi", 200, 0),
        Some(page("https://a.example/"))
    );
}
