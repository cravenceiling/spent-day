use super::*;
use crate::{app::Shared, demo::Demo, icons};
use rune::{Cx, Env, Runtime};
use std::sync::{Arc, Mutex, OnceLock};

fn runtime(scene: fn(&mut Dash), dark: bool) -> Runtime<Dash> {
    let shared = Arc::new(Shared {
        store: Arc::new(Demo::new()),
        demo: true,
        tz: jiff::tz::TimeZone::UTC,
        icons: icons::Seeds::default(),
        sync: Mutex::default(),
        ui: OnceLock::new(),
    });
    let mut d = Dash::new(shared, jiff::civil::date(2026, 10, 28));
    scene(&mut d);
    fn render(d: &Dash, cx: &mut Cx<Dash>) -> El {
        view(d, crate::ui::kit(cx.system_dark()))
    }
    let mut rt = Runtime::new(d, render, (720.0, 560.0), Env::new(dark, 1.0));
    rt.frame(0.0);
    rt
}
fn off(d: &mut Dash) {
    d.overlay = Overlay::Settings;
    d.settings.config = Some(None);
    d.settings.server = TextEdit::new("https://sync.example.com");
    d.settings.autostart = None;
    d.settings.safari = None;
}
fn on(d: &mut Dash) {
    off(d);
    d.settings.apply_config(Some(SyncConfig {
        server_url: "https://sync.example.com".into(),
        sync_code: CODE.into(),
    }));
}
const CODE: &str = "7KQ2-M3XP-R2D9-WF4H-B8TN-C6JV-Q1ZE-HY5G-4MRA-T0VD-9XKP-E3NW-J7FB-2QHS";
fn click(rt: &mut Runtime<Dash>, key: &str) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn url_validation_literals() {
    assert_eq!(
        server_problem("ftp://x"),
        Some(
            "Enter the full address, starting with https://. For example, https://sync.example.com."
        )
    );
    assert_eq!(
        server_problem("https://"),
        Some("That doesn't look like a server address. For example, https://sync.example.com.")
    );
    for address in [
        "",
        "   ",
        " https://sync.example.com/// \n",
        "http://localhost:8787",
        "http://[::1]:8787",
    ] {
        assert_eq!(server_problem(address), None, "{address}");
    }
    for address in [
        "https:///path",
        "https://a b",
        "https://host:bad",
        "https://[nope]",
    ] {
        assert!(server_problem(address).is_some(), "{address}");
    }
}
#[test]
fn masked_code_is_absent_from_elements_and_accessibility() {
    let rt = runtime(on, true);
    let elements = format!("{:?}", rt.elements());
    assert!(!elements.contains(CODE));
    assert!(!elements.contains("7KQ2"));
    assert!(elements.contains("••••-••••-••••-…"));
    assert!(!format!("{:?}", rt.accessibility_json()).contains("7KQ2"));
}
#[test]
fn reveal_has_two_lines_of_seven_groups_and_hide_removes_them() {
    let mut rt = runtime(on, true);
    click(&mut rt, "reveal-code");
    assert!(rt.state.settings.reveal_code);
    let lines = two_lines(CODE);
    assert_eq!(
        lines,
        "7KQ2-M3XP-R2D9-WF4H-B8TN-C6JV-Q1ZE\nHY5G-4MRA-T0VD-9XKP-E3NW-J7FB-2QHS"
    );
    assert!(rt.find("revealed-code").is_some());
    click(&mut rt, "reveal-code");
    assert!(rt.find("revealed-code").is_none());
}
#[test]
fn cancel_join_retains_text_and_error() {
    let mut rt = runtime(off, true);
    click(&mut rt, "begin-join");
    rt.update(|d, _| {
        d.settings
            .join
            .as_mut()
            .unwrap()
            .replace_text("retained-code");
        d.settings.error = Some("That sync code isn't valid.".into());
    });
    rt.frame(0.0);
    click(&mut rt, "cancel-join");
    assert!(rt.state.settings.join.is_none());
    click(&mut rt, "begin-join");
    assert_eq!(
        rt.state.settings.join.as_ref().unwrap().text,
        "retained-code"
    );
    assert_eq!(
        rt.state.settings.error.as_deref(),
        Some("That sync code isn't valid.")
    );
}
#[test]
fn save_server_only_appears_for_a_changed_value() {
    let mut rt = runtime(on, true);
    assert!(rt.find("save-server").is_none());
    rt.update(|d, _| {
        d.settings
            .server
            .replace_text(" https://sync.example.com/ ")
    });
    rt.frame(0.0);
    assert!(rt.find("save-server").is_some());
}
#[test]
fn turn_off_needs_confirmation_and_cancel_keeps_config() {
    let mut rt = runtime(on, true);
    click(&mut rt, "turn-off");
    assert!(rt.find("confirm-off").is_some());
    click(&mut rt, "cancel-off");
    assert!(!rt.state.settings.confirm_off);
    assert!(rt.state.settings.config.as_ref().unwrap().is_some());
}
#[test]
fn general_rows_only_appear_when_supported() {
    let mut rt = runtime(off, true);
    assert!(rt.find("autostart").is_none());
    assert!(rt.find("safari").is_none());
    rt.update(|d, _| {
        d.settings.autostart = Some(Autostart {
            on: true,
            note: None,
        });
        d.settings.safari = Some(false);
    });
    rt.frame(0.0);
    assert!(rt.find("autostart").is_some());
    assert!(rt.find("safari").is_some());
}
#[test]
fn copy_feedback_expires_after_1500_ms() {
    let mut rt = runtime(on, true);
    click(&mut rt, "copy-code");
    assert!(rt.state.settings.copied_until.unwrap() > Instant::now());
    assert!(format!("{:?}", rt.elements()).contains("Copied"));
    rt.update(|d, _| d.settings.copied_until = Some(Instant::now() - Duration::from_millis(1)));
    rt.frame(0.0);
    assert!(!format!("{:?}", rt.elements()).contains("Copied"));
}

#[test]
fn last_sync_uses_absolute_local_time_for_success_and_failure() {
    for (name, scene) in crate::scenes::DASH {
        let expected = match *name {
            "settings-on" => "At 14:32 · 18 sent, 342 received",
            "settings-failed" => {
                "Failed at 14:32. Couldn't reach the sync server or read its answer. Check the address and your connection."
            }
            _ => continue,
        };
        let rt = runtime(*scene, true);
        let label = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("last-sync"))
            .unwrap();
        assert_eq!(label.text.as_deref(), Some(expected));
    }
}

#[test]
fn settings_labels_and_destructive_button_pass_contrast_in_both_themes() {
    for dark in [true, false] {
        let rt = runtime(on, dark);
        let elements = rt.elements();
        for e in elements.iter().filter(|e| e.text.is_some()) {
            let Some(ink) = e.text_color else { continue };
            let background = elements
                .iter()
                .rev()
                .find(|parent| {
                    e.path.starts_with(&format!("{}/", parent.path)) && parent.fill.a == 1.0
                })
                .unwrap()
                .fill;
            let (a, b) = (ink.luminance(), background.luminance());
            let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(contrast >= 4.5, "{:?}: {contrast}, dark={dark}", e.text);
        }
        let button = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("turn-off"))
            .unwrap();
        assert!(button.fill.a > 0.0);
        assert!(button.fill.r > button.fill.g);
    }
}
