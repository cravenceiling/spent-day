//! The dashboard on a bare Rune runtime (no GPU), with the demo day in New York: the tile
//! geometry, keyboard focus through drills, the hover protocol and reload stability.

use super::*;
use crate::app::Shared;
use crate::demo::Demo;
use crate::ui::cells::{self, Measure};
use crate::ui::dash_view;
use jiff::civil::date;
use jiff::tz::TimeZone;
use rune::{Env, Mods, Runtime};
use spentday_core::{DayItem, ItemKind, Session};
use std::sync::{Mutex, OnceLock};

const MAC: &str = "demo-mac";
const XCODE: &str = "demo-mac/app:com.apple.dt.Xcode";
const GITHUB: &str = "demo-mac/site:github.com";
const FRAME: f64 = 1.0 / 60.0;

fn shared() -> Arc<Shared> {
    Arc::new(Shared {
        store: Arc::new(Demo::with_timezone(new_york())),
        demo: true,
        tz: new_york(),
        icons: Default::default(),
        sync: Mutex::default(),
        ui: OnceLock::new(),
    })
}

fn new_york() -> TimeZone {
    TimeZone::get("America/New_York").unwrap()
}

fn runtime(size: (f32, f32)) -> Runtime<Dash> {
    let mut dash = Dash::new(shared(), date(2026, 10, 28));
    dash.load_now();
    let mut rt = Runtime::new(dash, dash_view::view, size, Env::new(true, 1.0));
    rt.on_key = Some(Dash::on_key);
    rt.set_tab_visits_controls(true);
    settle(&mut rt, 0.5);
    rt
}

fn settle(rt: &mut Runtime<Dash>, seconds: f64) {
    for _ in 0..(seconds / FRAME).ceil() as usize {
        rt.frame(FRAME);
    }
}

fn key(rt: &mut Runtime<Dash>, key: Key, cmd: bool) {
    rt.key(KeyEvent {
        key,
        mods: Mods {
            cmd,
            ..Mods::default()
        },
    });
    settle(rt, 0.4);
}

fn focused(rt: &Runtime<Dash>) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| e.focused)
        .and_then(|e| e.key)
}

fn tile_keys(rt: &Runtime<Dash>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.key)
        .filter(|k| k.starts_with("tile-"))
        .collect()
}

fn target(rt: &Runtime<Dash>, key: &str) -> [f32; 4] {
    let e = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key));
    e.unwrap_or_else(|| panic!("no element {key}")).target
}

fn measure(t: &str, pt: f32, weight: f32) -> (f32, f32) {
    rune::text::measure(t, &[], rune::TextStyle::new(pt, weight), None)
}

/// Tab until `target` holds focus.
fn tab_to(rt: &mut Runtime<Dash>, target: &str) {
    for _ in 0..40 {
        if focused(rt).as_deref() == Some(target) {
            return;
        }
        key(rt, Key::Tab, false);
    }
    panic!("Tab never reached {target}; focus is on {:?}", focused(rt));
}

fn reload(rt: &mut Runtime<Dash>, edit: impl FnOnce(&mut DayDetail)) {
    let loaded = rt.state.loaded().unwrap();
    let mut detail = loaded.detail.clone();
    edit(&mut detail);
    let next = Loaded {
        query: loaded.query.clone(),
        usual_ms: loaded.usual_ms,
        palette: loaded.palette.clone(),
        detail,
    };
    let generation = rt.state.generation;
    rt.update(move |d, tx| {
        assert!(
            d.apply_load(generation, Ok(next), tx),
            "a changed day commits"
        );
    });
    settle(rt, 0.4);
}

/// Two short items on the MacBook, under 0.3 % of the day, so they fold into Other.
fn add_small_items(detail: &mut DayDetail) {
    let start = detail.start_ms + 17 * 3_600_000;
    let mac = detail
        .devices
        .iter_mut()
        .find(|d| d.device.id == MAC)
        .unwrap();
    for (i, name) in ["Calculator", "Preview"].into_iter().enumerate() {
        let start_ms = start + i as i64 * 600_000;
        mac.items.push(DayItem {
            key: format!("app:{name}"),
            name: name.into(),
            kind: ItemKind::App,
            app_key: name.into(),
            app_ids: vec![],
            total_ms: 60_000,
            sessions: vec![Session {
                start_ms,
                end_ms: start_ms + 60_000,
            }],
        });
    }
}

#[test]
fn tiles_sit_where_the_layout_puts_them() {
    for size in [
        (720.0, 560.0),
        (1280.0, 800.0),
        (1000.0, 700.0),
        (1700.0, 1100.0),
    ] {
        let rt = runtime(size);
        let [mx, my, mw, mh] = target(&rt, "map");
        let l = rt.state.loaded().unwrap();
        let m: Measure = &measure;
        let layout = cells::layout_in_timezone(
            &l.detail,
            &Drill::Day,
            &l.palette,
            [mw, mh],
            true,
            m,
            &new_york(),
        );
        let positive: Vec<_> = layout
            .cells
            .iter()
            .filter(|c| c.rect.w > 0.0 && c.rect.h > 0.0)
            .collect();
        assert_eq!(
            tile_keys(&rt).len(),
            positive.len(),
            "one tile per cell at {size:?}"
        );
        for cell in positive {
            let [x, y, w, h] = target(&rt, &format!("tile-{}", cell.id));
            let want = [
                mx + cell.rect.x as f32,
                my + cell.rect.y as f32,
                cell.rect.w as f32,
                cell.rect.h as f32,
            ];
            for (got, want) in [x, y, w, h].into_iter().zip(want) {
                assert!(
                    (got - want).abs() <= 0.5,
                    "{} at {size:?}: {got} != {want}",
                    cell.id
                );
            }
        }
    }
}

#[test]
fn pressing_the_first_tile_drills_and_focuses_its_first_session() {
    let mut rt = runtime((1280.0, 800.0));
    let first = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(&format!("tile-{XCODE}")[..]))
        .unwrap();
    assert_eq!(first.label.as_deref(), Some("Xcode, 6 hours 5 minutes"));
    assert!(
        rt.accessibility_json()
            .to_string()
            .contains("Opens sessions")
    );
    tab_to(&mut rt, &format!("tile-{XCODE}"));
    key(&mut rt, Key::Text(" ".into()), false);
    assert_eq!(
        rt.state.drill,
        Drill::Item {
            device: MAC.into(),
            key: "app:com.apple.dt.Xcode".into()
        }
    );
    assert_eq!(focused(&rt), Some(format!("tile-{XCODE}/0")));
    key(&mut rt, Key::Escape, false);
    assert_eq!(rt.state.drill, Drill::Device(MAC.into()));
    assert_eq!(
        focused(&rt),
        Some(format!("tile-{XCODE}")),
        "a pop focuses the tile it came from"
    );
    key(&mut rt, Key::Enter, false);
    assert!(
        matches!(rt.state.drill, Drill::Item { .. }),
        "Return presses the focused tile"
    );
}

#[test]
fn hover_opens_after_300_ms_switches_at_once_and_leaves_after_90() {
    let mut rt = runtime((1280.0, 800.0));
    let (x, y) = rt.find(&format!("tile-{XCODE}")).unwrap();
    rt.pointer_move(x, y);
    rt.frame(FRAME);
    assert_eq!(rt.state.hovered.as_deref(), Some(XCODE), "dims at once");
    assert_eq!(rt.state.popover, None);
    settle(&mut rt, 0.2);
    assert_eq!(rt.state.popover, None, "not before 300 ms");
    settle(&mut rt, 0.15);
    assert_eq!(rt.state.popover.as_deref(), Some(XCODE));
    let (gx, gy) = rt.find(&format!("tile-{GITHUB}")).unwrap();
    rt.pointer_move(gx, gy);
    rt.frame(FRAME);
    assert_eq!(
        rt.state.popover.as_deref(),
        Some(GITHUB),
        "an open card follows at once"
    );
    rt.pointer_move(4.0, 4.0);
    settle(&mut rt, 0.05);
    assert_eq!(rt.state.popover.as_deref(), Some(GITHUB), "90 ms grace");
    settle(&mut rt, 0.1);
    assert_eq!(rt.state.popover, None);
    assert_eq!(rt.state.hovered, None);

    rt.pointer_move(x, y);
    settle(&mut rt, 0.4);
    assert_eq!(rt.state.popover.as_deref(), Some(XCODE));
    let (cx, cy) = rt.find("popover").unwrap();
    rt.pointer_move(cx, cy);
    settle(&mut rt, 0.3);
    assert_eq!(
        rt.state.popover.as_deref(),
        Some(XCODE),
        "the card holds itself open"
    );
    rt.pointer_move(4.0, 4.0);
    settle(&mut rt, 0.2);
    assert_eq!(rt.state.popover, None, "leaving the card closes it");
}

#[test]
fn a_reload_keeps_the_card_and_focus() {
    let mut rt = runtime((1280.0, 800.0));
    tab_to(&mut rt, &format!("tile-{XCODE}"));
    key(&mut rt, Key::Right, false);
    assert_eq!(
        focused(&rt),
        Some(format!("tile-{GITHUB}")),
        "arrows rove in layout order"
    );
    let (x, y) = rt.find(&format!("tile-{XCODE}")).unwrap();
    rt.pointer_move(x, y);
    settle(&mut rt, 0.4);
    assert_eq!(rt.state.popover.as_deref(), Some(XCODE));
    reload(&mut rt, |d| {
        let mac = d.devices.iter_mut().find(|v| v.device.id == MAC).unwrap();
        mac.items[0].sessions[0].end_ms -= 60_000;
        mac.items[0].total_ms -= 60_000;
    });
    assert_eq!(rt.state.popover.as_deref(), Some(XCODE));
    assert_eq!(focused(&rt), Some(format!("tile-{GITHUB}")));
    let card = rt.presented("popover").unwrap();
    reload(&mut rt, |d| {
        let phone = d
            .devices
            .iter_mut()
            .find(|v| v.device.id == "demo-phone")
            .unwrap();
        phone.device.last_seen_ms += 1;
    });
    let after = rt.presented("popover").unwrap();
    assert_eq!(
        after.0, card.0,
        "a change elsewhere leaves the card where it was"
    );
    assert!(
        (after.1 - card.1).abs() < 0.01,
        "and does not fade it again"
    );
    assert_eq!(focused(&rt), Some(format!("tile-{GITHUB}")));
}

#[test]
fn other_opens_on_space_and_escape_steps_back() {
    let mut rt = runtime((1280.0, 800.0));
    reload(&mut rt, add_small_items);
    rt.update(|d, tx| d.drill_to(Drill::Device(MAC.into()), tx));
    settle(&mut rt, 0.5);
    let other = format!("tile-{MAC}/other");
    assert!(tile_keys(&rt).contains(&other));
    tab_to(&mut rt, &format!("tile-{XCODE}"));
    for _ in 0..12 {
        if focused(&rt).as_deref() == Some(&other[..]) {
            break;
        }
        key(&mut rt, Key::Right, false);
    }
    assert_eq!(focused(&rt), Some(other.clone()));
    rt.key(KeyEvent {
        key: Key::Text(" ".into()),
        mods: Mods::default(),
    });
    rt.frame(FRAME);
    assert_eq!(
        rt.state.popover.as_deref(),
        Some(&other["tile-".len()..]),
        "Other opens at once"
    );
    key(&mut rt, Key::Escape, false);
    assert_eq!(rt.state.popover, None);
    assert_eq!(focused(&rt), Some(other));
    key(&mut rt, Key::Escape, false);
    assert_eq!(rt.state.drill, Drill::Day);
}

#[test]
fn a_renamed_device_relabels_its_crumb_and_focus_stays() {
    let mut rt = runtime((1280.0, 800.0));
    rt.update(|d, tx| {
        d.drill_to(
            Drill::Item {
                device: MAC.into(),
                key: "app:com.apple.dt.Xcode".into(),
            },
            tx,
        )
    });
    settle(&mut rt, 0.5);
    tab_to(&mut rt, "crumb-device");
    let label = |rt: &Runtime<Dash>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("crumb-device"))
            .and_then(|e| e.label)
    };
    assert_eq!(label(&rt).as_deref(), Some("MacBook Pro"));
    reload(&mut rt, |d| {
        d.devices
            .iter_mut()
            .find(|v| v.device.id == MAC)
            .unwrap()
            .device
            .name = "MacBook Air".into()
    });
    assert_eq!(label(&rt).as_deref(), Some("MacBook Air"));
    assert_eq!(focused(&rt).as_deref(), Some("crumb-device"));
    reload(&mut rt, |d| d.devices[0].device.last_seen_ms += 1);
    assert_eq!(focused(&rt).as_deref(), Some("crumb-device"));
}

#[test]
fn day_keys_and_escape_at_the_top() {
    let mut rt = runtime((1280.0, 800.0));
    let handled = rt.update_if(|d, tx| {
        d.on_key(
            &KeyEvent {
                key: Key::Escape,
                mods: Mods::default(),
            },
            tx,
        )
    });
    assert!(!handled, "Escape at Day goes on");
    let today = day::today(&rt.state.shared.tz);
    rt.update(move |d, _| d.selected = today);
    key(&mut rt, Key::Right, true);
    assert_eq!(rt.state.selected, today, "never past today");
    rt.update(|d, tx| {
        d.drill_to(Drill::Device(MAC.into()), tx);
        d.details_open = true;
    });
    key(&mut rt, Key::Left, true);
    assert_eq!(rt.state.selected, day::step(today, -1));
    assert_eq!(rt.state.drill, Drill::Day);
    assert!(!rt.state.details_open);
}

fn configured_runtime() -> Runtime<Dash> {
    let demo = Demo::with_timezone(new_york());
    *demo.sync.lock().unwrap() = Some(spentday_core::SyncConfig {
        server_url: "https://sync.example.com".into(),
        sync_code: "7KQ2-M3XP".into(),
    });
    let shared = Arc::new(Shared {
        store: Arc::new(demo),
        demo: true,
        tz: new_york(),
        icons: Default::default(),
        sync: Mutex::default(),
        ui: OnceLock::new(),
    });
    let mut dash = Dash::new(shared, date(2026, 10, 28));
    dash.load_now();
    let mut rt = Runtime::new(dash, dash_view::view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.on_key = Some(Dash::on_key);
    settle(&mut rt, 0.5);
    rt
}

fn sync_status(rt: &Runtime<Dash>) -> Option<String> {
    let elements = rt.elements();
    let badge = elements
        .iter()
        .find(|e| e.key.as_deref() == Some("sync-status"))?;
    let prefix = format!("{}/", badge.path);
    elements
        .iter()
        .filter(|e| e.path.starts_with(&prefix))
        .find_map(|e| e.text.clone())
}

fn assert_on(rt: &Runtime<Dash>) {
    assert_eq!(rt.state.overlay, Overlay::Settings);
    assert_eq!(sync_status(rt).as_deref(), Some("On"));
    assert!(rt.find("create-code").is_none());
    assert!(rt.find("begin-join").is_none());
    assert!(rt.find("sync-now").is_some());
    assert!(rt.state.settings.sync.configured, "Sync Now is offered");
    assert!(rt.state.shared.sync.lock().unwrap().configured);
}

#[test]
fn settings_on_a_configured_store_shows_on_and_never_offers_create() {
    let mut rt = configured_runtime();
    rt.update(|d, tx| {
        // The load is still in flight.
        d.settings.working = true;
        crate::ui::settings::open(d, tx);
    });
    settle(&mut rt, 0.4);
    assert_eq!(rt.state.overlay, Overlay::Settings);
    assert_eq!(sync_status(&rt), None);
    assert!(rt.find("create-code").is_none());
    assert!(rt.find("begin-join").is_none());
    assert!(rt.find("server").is_none());

    let mut rt = configured_runtime();
    key(&mut rt, Key::Command(','), true);
    assert_on(&rt);

    let mut rt = configured_runtime();
    let (x, y) = rt.find("menu-button").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt, 0.4);
    let item = rt
        .elements()
        .into_iter()
        .rfind(|e| e.text.as_deref() == Some("Settings…") && !e.exiting)
        .unwrap();
    let (x, y) = (
        item.rect[0] + item.rect[2] / 2.0,
        item.rect[1] + item.rect[3] / 2.0,
    );
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt, 0.4);
    assert_on(&rt);
}
