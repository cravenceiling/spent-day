use super::*;
#[test]
fn recorded_assertions_count_only_the_front_app() {
    let assertions = HashMap::from([
        (
            92255,
            vec![Assertion {
                kind: "PreventUserIdleDisplaySleep".into(),
                on_behalf_of: None,
            }],
        ),
        (
            342,
            vec![Assertion {
                kind: "PreventUserIdleSystemSleep".into(),
                on_behalf_of: None,
            }],
        ),
        (
            401,
            vec![Assertion {
                kind: "UserIsActive".into(),
                on_behalf_of: None,
            }],
        ),
    ]);
    assert!(holds_display_awake(92255, &assertions));
    for pid in [777, 342, 401] {
        assert!(!holds_display_awake(pid, &assertions));
    }
    let behalf = HashMap::from([(
        500,
        vec![Assertion {
            kind: "PreventUserIdleDisplaySleep".into(),
            on_behalf_of: Some(777),
        }],
    )]);
    assert!(holds_display_awake(777, &behalf));
    assert!(!holds_display_awake(778, &behalf));
    let legacy = HashMap::from([(
        777,
        vec![Assertion {
            kind: "NoDisplaySleepAssertion".into(),
            on_behalf_of: None,
        }],
    )]);
    assert!(holds_display_awake(777, &legacy));
}
#[test]
fn last_seen_literals() {
    let minute = 60_000;
    let mut seen = LastSeen::default();
    assert_eq!(seen.away_since(100 * minute, 98 * minute, || false), None);
    assert_eq!(
        seen.away_since(100 * minute, 96 * minute, || false),
        Some(96 * minute)
    );
    let mut seen = LastSeen::default();
    assert_eq!(seen.away_since(100 * minute, 50 * minute, || true), None);
    assert_eq!(seen.away_since(102 * minute, 50 * minute, || false), None);
    assert_eq!(
        seen.away_since(104 * minute, 50 * minute, || false),
        Some(100 * minute)
    );
    assert_eq!(seen.away_since(105 * minute, 104 * minute, || false), None);
    let mut seen = LastSeen::default();
    let mut away = None;
    for now in (5_000..=400_000).step_by(5_000) {
        away = seen.away_since(now, 0, || now <= 175_000);
        if now == 185_000 {
            assert_eq!(away, None);
        }
        if away.is_some() {
            break;
        }
    }
    assert_eq!(away, Some(175_000));
    let mut seen = LastSeen::default();
    let mut away = None;
    for now in (minute..=200 * minute).step_by(5_000) {
        away = seen.away_since(now, 0, || true);
        if away.is_some() {
            break;
        }
    }
    assert_eq!(away, Some(7_200_000));
    assert_eq!(LastSeen::default().away_since(180_000, 0, || false), None);
    assert_eq!(
        LastSeen::default().away_since(7_200_001, 0, || panic!("query past cap")),
        Some(0)
    );
}
#[test]
#[ignore = "queries the live power assertions and starts caffeinate"]
fn live_caffeinate() {
    let mut child = std::process::Command::new("/usr/bin/caffeinate")
        .arg("-d")
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let live = current_assertions();
    let observed = holds_display_awake(child.id() as i32, &live);
    let finder = objc2_app_kit::NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str("com.apple.finder"),
    );
    let finder_held = finder
        .firstObject()
        .is_some_and(|app| holds_display_awake(app.processIdentifier(), &live));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(observed, "caffeinate -d is seen");
    assert!(
        !finder_held,
        "Finder does not inherit caffeinate's assertion"
    );
}
