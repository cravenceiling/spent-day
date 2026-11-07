//! Framework proof for treemap keyboard focus, on a bare Rune runtime (no GPU): Tab entry,
//! arrow roving with wrap, Return and Space activation, and where focus lands after a drill.

use rune::{Cx, El, Env, Key, KeyEvent, Mods, Runtime, Transition, Tx, el};

#[derive(Default)]
struct Map {
    /// The tile drilled into; `None` is the overview.
    drilled: Option<String>,
    pressed: Vec<String>,
    /// Tiles get an exit transition, as the dashboard's drill animation would.
    exit_transition: bool,
    /// The tiles sit in a page element whose key changes per drill level.
    keyed_page: bool,
    /// The drilled level still shows the pressed tile, as a device drill keeps its tiles.
    keep_pressed: bool,
}

const OVERVIEW: [&str; 3] = ["a", "b", "c"];

fn activate(s: &mut Map, key: &str) {
    s.pressed.push(key.to_owned());
    if s.drilled.is_none() {
        s.drilled = Some(key.trim_start_matches("tile-").to_owned());
    }
}

fn tile(s: &Map, id: String, i: usize) -> El {
    let key = format!("tile-{id}");
    let pressed = key.clone();
    let mut t = el()
        .key(key)
        .role("button")
        .label(id)
        .focusable(true)
        .focus_selected(i == 0)
        .focus_return("map")
        .absolute(i as f32 * 110.0, 0.0)
        .size(100.0, 80.0)
        .on_press(move |s: &mut Map| activate(s, &pressed));
    if s.exit_transition {
        t = t.transition(Transition::opacity());
    }
    t
}

fn view(s: &Map, _: &mut Cx<Map>) -> El {
    let ids: Vec<String> = match &s.drilled {
        None => OVERVIEW.iter().map(|id| id.to_string()).collect(),
        Some(parent) => {
            let mut ids: Vec<String> = (0..3).map(|i| format!("{parent}/{i}")).collect();
            if s.keep_pressed {
                ids.push(parent.clone());
            }
            ids
        }
    };
    let tiles = ids.into_iter().enumerate().map(|(i, id)| tile(s, id, i));
    let map = el()
        .key("map")
        .role("group")
        .label("Treemap")
        .size(400.0, 100.0)
        .focus_group()
        // Return is not a press for a plain group's items; the map hears it with the focused key.
        .on_key(|s: &mut Map, ev: &KeyEvent, tx: &mut Tx| {
            let Some(focused) = tx
                .focused()
                .filter(|k| k.starts_with("tile-"))
                .map(str::to_owned)
            else {
                return false;
            };
            if ev.key != Key::Enter {
                return false;
            }
            activate(s, &focused);
            true
        });
    if s.keyed_page {
        let page = el()
            .key(format!("page-{}", s.drilled.as_deref().unwrap_or("day")))
            .size(400.0, 100.0)
            .focus_return("map");
        el().w_full()
            .h_full()
            .child(map.child(page.children(tiles)))
    } else {
        el().w_full().h_full().child(map.children(tiles))
    }
}

fn runtime(state: Map) -> Runtime<Map> {
    let mut rt = Runtime::new(state, view, (500.0, 200.0), Env::new(true, 2.0));
    rt.frame(0.0);
    rt
}

fn press(rt: &mut Runtime<Map>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

fn focused(rt: &Runtime<Map>) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| e.focused)
        .and_then(|e| e.key)
}

#[test]
fn tab_skips_plain_tiles_unless_tab_visits_controls() {
    let mut rt = runtime(Map::default());
    // A test runtime starts with Full Keyboard Access on; a Mac window follows the system
    // setting, which is off by default.
    rt.set_full_keyboard_access(false);
    press(&mut rt, Key::Tab);
    assert_eq!(
        focused(&rt),
        None,
        "macOS default: Tab visits only fields and lists"
    );
    rt.set_tab_visits_controls(true);
    press(&mut rt, Key::Tab);
    assert_eq!(
        focused(&rt).as_deref(),
        Some("tile-a"),
        "Tab enters at the focus_selected tile"
    );
}

#[test]
fn arrows_rove_and_wrap_in_a_plain_group() {
    let mut rt = runtime(Map::default());
    rt.set_tab_visits_controls(true);
    press(&mut rt, Key::Tab);
    let mut seen = Vec::new();
    for key in [
        Key::Right,
        Key::Right,
        Key::Right,
        Key::Left,
        Key::Down,
        Key::Up,
        Key::Up,
    ] {
        press(&mut rt, key);
        seen.push(focused(&rt).unwrap());
    }
    assert_eq!(
        seen,
        [
            "tile-b", "tile-c", "tile-a", "tile-c", "tile-a", "tile-c", "tile-b"
        ]
    );
}

#[test]
fn space_and_return_activate_the_focused_tile() {
    let mut rt = runtime(Map::default());
    rt.set_tab_visits_controls(true);
    press(&mut rt, Key::Tab);
    press(&mut rt, Key::Right);
    press(&mut rt, Key::Text(" ".into()));
    assert_eq!(rt.state.pressed, ["tile-b"], "Space presses via on_press");
    rt.state.drilled = None;
    rt.frame(0.0);
    press(&mut rt, Key::Tab);
    press(&mut rt, Key::Right);
    press(&mut rt, Key::Enter);
    assert_eq!(
        rt.state.pressed,
        ["tile-b", "tile-b"],
        "Return reaches the map's on_key with tx.focused()"
    );
}

/// The pressed tile leaves the tree; its `focus_return("map")` moves focus to the map's
/// `focus_selected` tile, which is the new page's first tile.
#[test]
fn focus_lands_on_the_first_tile_after_a_drill() {
    let mut rt = runtime(Map::default());
    rt.set_tab_visits_controls(true);
    press(&mut rt, Key::Tab);
    press(&mut rt, Key::Right);
    press(&mut rt, Key::Text(" ".into()));
    assert_eq!(rt.state.drilled.as_deref(), Some("b"));
    assert_eq!(focused(&rt).as_deref(), Some("tile-b/0"));
    press(&mut rt, Key::Right);
    assert_eq!(
        focused(&rt).as_deref(),
        Some("tile-b/1"),
        "arrows rove on the new page"
    );
}

#[test]
fn focus_lands_on_the_first_tile_with_exit_transitions() {
    let mut rt = runtime(Map {
        exit_transition: true,
        ..Map::default()
    });
    rt.set_tab_visits_controls(true);
    press(&mut rt, Key::Tab);
    press(&mut rt, Key::Right);
    press(&mut rt, Key::Text(" ".into()));
    assert_eq!(rt.state.drilled.as_deref(), Some("b"));
    assert_eq!(focused(&rt).as_deref(), Some("tile-b/0"));
    for _ in 0..120 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(
        focused(&rt).as_deref(),
        Some("tile-b/0"),
        "focus holds once the exits finish"
    );
}

/// A drill where the focused element stays (Day to Device keeps the device's tiles): focus
/// stays on it, since nothing was removed.
#[test]
fn focus_stays_on_a_tile_that_survives_the_drill() {
    let mut rt = runtime(Map {
        keep_pressed: true,
        ..Map::default()
    });
    rt.set_tab_visits_controls(true);
    press(&mut rt, Key::Tab);
    press(&mut rt, Key::Right);
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.drilled.as_deref(), Some("b"));
    assert_eq!(focused(&rt).as_deref(), Some("tile-b"));
}

/// The same drill with a page element keyed per level: every old path vanishes, so the page's
/// `focus_return` fires and focus lands on the new first tile.
#[test]
fn a_keyed_page_moves_focus_even_when_the_focused_tile_survives() {
    let mut rt = runtime(Map {
        keyed_page: true,
        keep_pressed: true,
        ..Map::default()
    });
    rt.set_tab_visits_controls(true);
    press(&mut rt, Key::Tab);
    press(&mut rt, Key::Right);
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.drilled.as_deref(), Some("b"));
    assert_eq!(focused(&rt).as_deref(), Some("tile-b/0"));
}
