//! `MapLayout` to elements: device headers, tiles, the focus group, hover and the popover card.
//! Every tile's colour treatment lives in `paint`, fed by `look`.

use crate::colors::Rgb;
use crate::format::duration;
use crate::palette::Palette;
use crate::ui::cells::{Cell, CellKind, Label, MapLayout};
use crate::ui::dash::interim::{self, day};
use crate::ui::dash::{CellId, Dash, Drill, Loaded};
use crate::ui::dash_view::Page;
use crate::ui::strip;
use rune::{
    Align, Color, El, Key, KeyEvent, ScrollSpec, Shadow, Side, Spring, TextStyle, Transition, Tx,
    col, el, row, text,
};
use std::f32::consts::PI;
use std::time::Duration;

/// One tile's paint: the gradient's top and bottom stops and the label ink.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub top: Rgb,
    pub bottom: Rgb,
    pub ink: Rgb,
}

/// sRGB `Rgb` to Rune's linear `Color`.
pub fn color(c: Rgb) -> Color {
    let lin = |v: f64| {
        let v = v.clamp(0.0, 1.0);
        (if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }) as f32
    };
    Color {
        r: lin(c.r),
        g: lin(c.g),
        b: lin(c.b),
        a: 1.0,
    }
}

/// The colours for a palette key; `dimmed` while another tile is hovered.
pub fn look(palette: &Palette, key: &str, dark: bool, dimmed: bool) -> Look {
    // INTERIM: unit 1 moves direction B's maths onto `Palette`; read it from there at merge.
    let _ = palette;
    interim::look(key, dark, dimmed)
}

/// The palette key a cell is painted with.
pub fn palette_key(cell: &Cell) -> &str {
    match &cell.kind {
        CellKind::Item { key, .. } | CellKind::Session { key, .. } => key,
        CellKind::Other { .. } => "other",
    }
}

pub fn device_of(cell: &Cell) -> &str {
    match &cell.kind {
        CellKind::Item { device, .. }
        | CellKind::Session { device, .. }
        | CellKind::Other { device, .. } => device,
    }
}

/// Direction B, jewel gradient: a vertical gradient between the two stops, a top hairline and
/// an inner glow; the hovered tile lifts on a glow of its own colour.
fn paint(e: El, look: Look, radius: f32, hovered: bool, dimmed: bool, dark: bool) -> El {
    let (a, b) = (color(look.top), color(look.bottom));
    let mut e = e
        .radius(radius)
        .fill(b)
        .linear_gradient(PI, [(0.0, a), (1.0, b)])
        .box_shadow(Shadow {
            y: 1.0,
            color: Color::WHITE.alpha(if dimmed { 0.10 } else { 0.30 }),
            inset: true,
            ..Shadow::default()
        })
        .box_shadow(Shadow {
            blur: 22.0,
            color: Color::WHITE.alpha(if dimmed { 0.05 } else { 0.14 }),
            inset: true,
            ..Shadow::default()
        });
    if !dark && !dimmed {
        e = e.shadow(2.0, 6.0, b.alpha(0.25));
    }
    if hovered {
        e = e.shadow(10.0, 30.0, a.alpha(0.6));
    }
    e
}

pub fn tile_key(id: &str) -> String {
    format!("tile-{id}")
}

/// Pressing a tile or header (pointer, Space, or Return through the map's `on_key`).
fn activate(d: &mut Dash, key: &str, tx: &mut Tx) {
    if let Some(device) = key.strip_prefix("hdr-") {
        d.drill_to(Drill::Device(device.to_owned()), tx);
        return;
    }
    let Some(id) = key.strip_prefix("tile-") else {
        return;
    };
    if id.ends_with("/other") {
        if d.popover.as_deref() == Some(id) {
            d.popover = None;
        } else {
            d.popover_open(id.to_owned(), tx);
        }
    } else if !matches!(d.drill, Drill::Item { .. })
        && let Some((device, key)) = id.split_once('/')
    {
        d.drill_to(
            Drill::Item {
                device: device.to_owned(),
                key: key.to_owned(),
            },
            tx,
        );
    }
}

fn page_key(drill: &Drill) -> String {
    match drill {
        Drill::Day => "page-day".into(),
        Drill::Device(d) => format!("page-dev-{d}"),
        Drill::Item { device, key } => format!("page-item-{device}/{key}"),
    }
}

/// The cell whose tiles and strip sessions stay bright: the hovered one, else the popover's.
pub fn hot(s: &Dash) -> Option<&CellId> {
    s.hovered.as_ref().or(s.popover.as_ref())
}

/// The label text a session tile shows instead of the item's name.
fn shown_name(cell: &Cell, loaded: &Loaded, s: &Dash) -> String {
    match (&cell.kind, cell.sessions.first()) {
        (CellKind::Session { .. }, Some(session)) => {
            let end = loaded.detail.end_ms;
            let tz = &s.shared.tz;
            format!(
                "{} to {}",
                day::clock(session.start_ms, end, tz),
                day::clock(session.end_ms, end, tz)
            )
        }
        _ => cell.name.clone(),
    }
}

fn tile(s: &Dash, loaded: &Loaded, cell: &Cell, selected: bool, dark: bool) -> El {
    let [x, y, w, h] = [cell.rect.x, cell.rect.y, cell.rect.w, cell.rect.h].map(|v| v as f32);
    let hot = hot(s);
    let hovered = hot == Some(&cell.id);
    let dimmed = hot.is_some() && !hovered;
    let look = look(&loaded.palette, palette_key(cell), dark, dimmed);
    let ink = color(look.ink);
    let name = shown_name(cell, loaded, s);
    let key = tile_key(&cell.id);
    let (label, description) = match &cell.kind {
        CellKind::Session { .. } => (
            format!("{}, {}", name, day::accessible_duration(cell.total_ms)),
            "Session",
        ),
        CellKind::Other { .. } => (
            format!("Other, {}", day::accessible_duration(cell.total_ms)),
            "Shows hidden items",
        ),
        CellKind::Item { .. } => (
            format!("{}, {}", cell.name, day::accessible_duration(cell.total_ms)),
            "Opens sessions",
        ),
    };
    let other = matches!(cell.kind, CellKind::Other { .. });
    // A popover already open follows the pointer at once; Other has nothing to wait for.
    let enter = if s.popover.is_some() || other { 0 } else { 300 };
    let (enter_id, change_id, press_key) = (cell.id.clone(), cell.id.clone(), key.clone());
    let mut t = el()
        .key(key)
        .matched(cell.id.clone())
        .role("button")
        .label(label)
        .description(description)
        .focusable(true)
        .focus_selected(selected)
        .focus_return("map")
        .absolute(x, y)
        .size(w, h)
        .scale(if hovered { 1.012 } else { 1.0 })
        .press_scale(0.985)
        .transition(Transition::opacity().combined(Transition::scale(0.97)))
        .on_hover(move |d: &mut Dash| d.hover_enter(enter_id.clone()))
        .hover_delays(Duration::from_millis(enter), Duration::from_millis(90))
        .on_hover_change(move |d: &mut Dash, inside, tx| {
            if inside {
                d.popover_open(change_id.clone(), tx);
            } else {
                d.hover_leave(change_id.clone(), tx);
            }
        });
    if !matches!(cell.kind, CellKind::Session { .. }) {
        t = t.on_press_tx(move |d: &mut Dash, tx| activate(d, &press_key, tx));
    }
    t = paint(t, look, cell.radius, hovered, dimmed, dark).focus_ring(2.0, -4.0, ink);
    let dur = duration(cell.total_ms);
    match cell.label {
        Label::Both {
            pad,
            name_pt,
            dur_pt,
        } => {
            t = t
                .child(
                    text(name)
                        .font(TextStyle::new(name_pt, 650.0))
                        .color(ink)
                        .absolute(pad, pad - 2.0),
                )
                .child(
                    text(dur)
                        .font(TextStyle::new(dur_pt, 500.0))
                        .color(ink.alpha(0.86))
                        .absolute(pad, pad - 2.0 + name_pt * 1.3),
                );
        }
        Label::NameOnly => {
            t = t.child(
                text(name)
                    .font(TextStyle::new(13.0, 650.0))
                    .color(ink)
                    .absolute(12.0, 9.0),
            );
        }
        Label::None => {}
    }
    if h > 220.0 {
        // Big tiles hold their popover under the label instead of past their far edge.
        t = t.child(
            el().key(format!("pop-at-{}", cell.id))
                .absolute(16.0, 76.0)
                .size(1.0, 0.0),
        );
    }
    t
}

fn header(s: &Dash, h: &crate::ui::cells::Header, selected: bool, page: &Page) -> El {
    let r = &h.rect;
    let key = format!("hdr-{}", h.device);
    let press = key.clone();
    let dim = hot(s).is_some();
    row()
        .key(key)
        .role("button")
        .label(format!(
            "{}, {}. Open device",
            h.name,
            day::accessible_duration(h.total_ms)
        ))
        .focusable(true)
        .focus_selected(selected)
        .focus_return("map")
        .absolute(r.x as f32 - 6.0, r.y as f32 - 4.0)
        .h(24.0)
        .max_w(r.w as f32 + 12.0)
        .px(6.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .hover(page.hover)
        .opacity(if dim { 0.55 } else { 1.0 })
        .transition(Transition::opacity())
        .on_press_tx(move |d: &mut Dash, tx| activate(d, &press, tx))
        .child(
            text(h.name.clone())
                .font(TextStyle::new(13.0, 650.0))
                .color(page.text)
                .truncate(),
        )
        .child(
            text(duration(h.total_ms))
                .font(TextStyle::new(13.0, 500.0))
                .color(page.sub),
        )
}

/// The map: a focus group holding one page per drill level, so a drill moves focus to the new
/// page's selected tile, and tiles shared between levels fly between their rects.
pub fn map(s: &Dash, layout: &MapLayout, dark: bool) -> El {
    let page = Page::of(dark);
    let loaded = s.loaded();
    let mut children = Vec::new();
    if let Some(loaded) = loaded {
        let keys: Vec<String> = layout
            .headers
            .iter()
            .map(|h| format!("hdr-{}", h.device))
            .chain(layout.cells.iter().map(|c| tile_key(&c.id)))
            .collect();
        let want = s.focus_after_drill.as_ref().filter(|k| keys.contains(k));
        let first = layout.cells.first().map(|c| tile_key(&c.id));
        let selected = |k: &str| want.map_or(first.as_deref() == Some(k), |w| w == k);
        for h in &layout.headers {
            children.push(header(s, h, selected(&format!("hdr-{}", h.device)), &page));
            for cell in layout.cells.iter().filter(|c| device_of(c) == h.device) {
                children.push(tile(s, loaded, cell, selected(&tile_key(&cell.id)), dark));
            }
        }
        if layout.headers.is_empty() {
            for cell in &layout.cells {
                children.push(tile(s, loaded, cell, selected(&tile_key(&cell.id)), dark));
            }
        }
    }
    let page_el = el()
        .key(page_key(&s.drill))
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .focus_return("map")
        .transition(Transition::opacity())
        .children(children);
    el().key("map")
        .role("group")
        .label("Treemap")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .focus_group()
        .on_key(|d: &mut Dash, ev: &KeyEvent, tx: &mut Tx| {
            if ev.key != Key::Enter {
                return false;
            }
            let Some(focused) = tx.focused().map(str::to_owned) else {
                return false;
            };
            activate(d, &focused, tx);
            true
        })
        .child(page_el)
}

/// The anchored card for `s.popover`, mounted at the root: sessions in time order, or the
/// items folded into Other.
pub fn popover(s: &Dash, layout: &MapLayout, map_w: f32, dark: bool) -> Option<El> {
    let id = s.popover.as_ref()?;
    let loaded = s.loaded()?;
    let cell = layout.cells.iter().find(|c| &c.id == id)?;
    let page = Page::of(dark);
    let tz = &s.shared.tz;
    let end = loaded.detail.end_ms;
    let device = loaded
        .detail
        .devices
        .iter()
        .find(|d| d.device.id == device_of(cell))
        .map_or("", |d| d.device.name.as_str());
    let look = look(&loaded.palette, palette_key(cell), dark, false);
    let rows: Vec<(String, i64)> = match &cell.kind {
        CellKind::Other { hidden, .. } => hidden.clone(),
        _ => cell
            .sessions
            .iter()
            .map(|x| {
                (
                    format!(
                        "{} to {}",
                        day::clock(x.start_ms, end, tz),
                        day::clock(x.end_ms, end, tz)
                    ),
                    x.end_ms - x.start_ms,
                )
            })
            .collect(),
    };
    let title = match cell.kind {
        CellKind::Other { .. } => "Other items".to_string(),
        _ => format!("{} sessions", cell.name),
    };
    let width = map_w.min(264.0);
    let list = col()
        .key("popover-rows")
        .gap(0.0)
        .children(rows.into_iter().enumerate().map(|(i, (name, ms))| {
            row()
                .key(format!("row-{i}"))
                .h(22.0)
                .center_y()
                .gap(12.0)
                .child(
                    text(name)
                        .font(TextStyle::new(13.0, 450.0))
                        .color(page.text)
                        .truncate()
                        .grow(1.0),
                )
                .child(
                    text(duration(ms))
                        .font(TextStyle::new(13.0, 500.0))
                        .color(page.sub),
                )
        }));
    let list = if cell.sessions.len() > 5
        || matches!(&cell.kind, CellKind::Other { hidden, .. } if hidden.len() > 5)
    {
        list.h(5.0 * 22.0).scroll(ScrollSpec {
            stick_bottom: false,
        })
    } else {
        list
    };
    let (anchor, side_gap, align) = if cell.rect.h > 220.0 {
        (format!("pop-at-{}", cell.id), 0.0, Align::Start)
    } else {
        (tile_key(&cell.id), 8.0, Align::Center)
    };
    let held = |d: &mut Dash, inside: bool, tx: &mut Tx| d.popover_hold(inside, tx);
    Some(
        col()
            .key("popover")
            .role("dialog")
            .label(title)
            .w(width)
            .padding(14.0)
            .gap(4.0)
            .radius(12.0)
            .fill(page.surface)
            .border(1.0, page.border)
            .shadow(
                12.0,
                32.0,
                Color::BLACK.alpha(if dark { 0.45 } else { 0.18 }),
            )
            .anchored_fit(anchor, Side::Bottom, align, side_gap)
            .transition(Transition::opacity().combined(Transition::scale(0.96)))
            .motion(Spring::new(0.3, 0.1))
            .hover_delays(Duration::ZERO, Duration::from_millis(90))
            .on_hover_change(held)
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(
                        el().size(10.0, 10.0)
                            .radius(3.0)
                            .fill(color(look.bottom))
                            .linear_gradient(
                                PI,
                                [(0.0, color(look.top)), (1.0, color(look.bottom))],
                            ),
                    )
                    .child(
                        text(cell.name.clone())
                            .font(TextStyle::new(14.0, 650.0))
                            .color(page.text)
                            .truncate(),
                    ),
            )
            .child(
                text(format!("{} on {device}", duration(cell.total_ms)))
                    .font(TextStyle::new(12.5, 450.0))
                    .color(page.sub),
            )
            .child(
                el().pt(8.0)
                    .pb(6.0)
                    .child(strip::mini(loaded, cell, width - 28.0, tz, dark)),
            )
            .child(list),
    )
}
