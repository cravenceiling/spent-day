//! The dashboard view: header, day navigation, headline, sentence, breadcrumbs, banners, the
//! treemap and the overlays (menu, About, onboarding, settings, the hover card).

use crate::format::duration;
use crate::platform::{self, Pref};
use crate::sync::{self, Trigger};
use crate::ui::cells::{self, MapLayout, Measure};
use crate::ui::dash::{Dash, Drill, Loaded, Overlay};
use crate::ui::treemap::{self, color};
use crate::ui::{bg, details, kit, settings, strip};
use crate::{day, palette, stats};
use rune::{
    Chord, Color, Cx, El, Icon, Shadow, TextStyle, Transition, Tx, col, el, icon, oklch, row, text,
};
use rune_kit::{Menu, MenuItem, Placement};
use std::f32::consts::PI;

/// Page colours for direction B's tiles: a faintly violet near-black or near-white.
pub struct Page {
    pub bg: Color,
    pub text: Color,
    pub sub: Color,
    pub faint: Color,
    pub track: Color,
    pub hover: Color,
    pub panel: Color,
    pub surface: Color,
    pub border: Color,
}

impl Page {
    pub fn of(dark: bool) -> Page {
        if dark {
            Page {
                bg: bg(dark),
                text: color(palette::page_ink(dark)),
                sub: color(palette::secondary_ink(dark)),
                faint: oklch(0.60, 0.01, 285.0),
                track: Color::WHITE.alpha(0.06),
                hover: Color::WHITE.alpha(0.08),
                panel: oklch(0.205, 0.013, 285.0),
                surface: oklch(0.235, 0.014, 285.0),
                border: Color::WHITE.alpha(0.08),
            }
        } else {
            Page {
                bg: bg(dark),
                text: color(palette::page_ink(dark)),
                sub: color(palette::secondary_ink(dark)),
                faint: oklch(0.56, 0.01, 285.0),
                track: Color::BLACK.alpha(0.06),
                hover: Color::BLACK.alpha(0.05),
                panel: Color::WHITE,
                surface: Color::WHITE,
                border: Color::BLACK.alpha(0.08),
            }
        }
    }
}

/// The accent gradient (Xcode's jewel azure) for primary buttons and the app glyph.
fn accent(e: El) -> El {
    let (a, b) = (oklch(0.565, 0.189, 258.0), oklch(0.49, 0.189, 266.0));
    e.fill(b)
        .linear_gradient(PI, [(0.0, a), (1.0, b)])
        .box_shadow(Shadow {
            y: 1.0,
            color: Color::WHITE.alpha(0.3),
            inset: true,
            ..Shadow::default()
        })
}

const fn svg(name: &'static str, body: &'static str) -> Icon {
    Icon {
        name,
        view: 16.0,
        body,
    }
}
static PREV: Icon = svg(
    "spentday-prev",
    r#"<path d="M10 3.5L5.5 8L10 12.5" stroke="currentColor" stroke-width="1.6" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
static NEXT: Icon = svg(
    "spentday-next",
    r#"<path d="M6 3.5L10.5 8L6 12.5" stroke="currentColor" stroke-width="1.6" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
static MORE: Icon = svg(
    "spentday-more",
    r#"<circle cx="3.5" cy="8" r="1.4" fill="currentColor"/><circle cx="8" cy="8" r="1.4" fill="currentColor"/><circle cx="12.5" cy="8" r="1.4" fill="currentColor"/>"#,
);
static HOURGLASS: Icon = svg(
    "spentday-hourglass",
    r#"<path d="M4 2h8M4 14h8M5 2c0 3 3 4 3 6s-3 3-3 6M11 2c0 3-3 4-3 6s3 3 3 6" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
);

fn measure(t: &str, pt: f32, weight: f32) -> (f32, f32) {
    rune::text::measure(t, &[], TextStyle::new(pt, weight), None)
}

pub fn view(s: &Dash, cx: &mut Cx<Dash>) -> El {
    let dark = cx.system_dark();
    let k = kit(dark);
    cx.reduced_motion(s.force_reduced || cx.system_reduced_motion());
    let page = Page::of(dark);
    let [map_w, map_h] = cx.layout_size("map").unwrap_or([1224.0, 520.0]);
    let measure: Measure = &measure;
    let layout = s.loaded().map_or_else(MapLayout::default, |l| {
        cells::layout_in_timezone(
            &l.detail,
            &s.drill,
            &l.palette,
            [map_w, map_h],
            dark,
            measure,
            &s.shared.tz,
        )
    });
    let mut layer = Vec::new();
    let mut base = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(page.bg)
        .px(28.0)
        .pt(14.0)
        .pb(20.0)
        .child(header(s, &page, k, &mut layer));
    if let Some(e) = &s.error {
        base = base.child(banner(e, &page));
    }
    base = match s.loaded() {
        None => base.children(loading(&page)),
        Some(l) => base.children(body(s, l, &layout, map_w, &page, dark)),
    };
    let mut overlays = layer;
    if let Some(card) = treemap::popover(s, &layout, map_w, dark) {
        overlays.push(card);
    }
    match s.overlay {
        Overlay::About => overlays.push(modal("about", about(&page), &page)),
        Overlay::Onboarding => overlays.push(modal("onboarding", onboarding(&page), &page)),
        Overlay::Settings => overlays.push(settings::view(s, k)),
        Overlay::None | Overlay::Menu => {}
    }
    rune_kit::root(base, overlays).on_window_focus(|d: &mut Dash, focused, tx: &mut Tx| {
        if focused {
            d.refresh(tx);
        }
    })
}

fn icon_button(key: &str, glyph: &'static Icon, label: &str, page: &Page) -> El {
    el().key(key)
        .role("button")
        .label(label)
        .focusable(true)
        .size(30.0, 30.0)
        .center()
        .radius(8.0)
        .hover(page.hover)
        .press(page.track)
        .child(icon(glyph, 16.0).color(page.text))
}

fn header(s: &Dash, page: &Page, k: &rune_kit::Kit, layer: &mut Vec<El>) -> El {
    let today = day::today(&s.shared.tz);
    let at_today = s.selected >= today;
    let prev = icon_button("day-prev", &PREV, "Previous day", page)
        .help(format!("Previous day ({})", chord_label("mod+left")))
        .on_press_tx(|d: &mut Dash, tx| {
            d.step_day(-1, tx);
        });
    let mut next = icon_button("day-next", &NEXT, "Next day", page)
        .help(format!("Next day ({})", chord_label("mod+right")));
    next = if at_today {
        next.disabled(true).opacity(0.3)
    } else {
        next.on_press_tx(|d: &mut Dash, tx| {
            d.step_day(1, tx);
        })
    };
    let mut bar = row()
        .key("header")
        .w_full()
        .h(36.0)
        .center_y()
        .gap(2.0)
        .child(prev)
        .child(
            text(day::header_date(s.selected))
                .key("date")
                .font(TextStyle::new(15.0, 600.0))
                .color(page.text)
                .px(6.0),
        )
        .child(next);
    if !at_today {
        let back = s.selected.until(today).map_or(0, |span| span.get_days());
        bar = bar.child(chip("today", "Today", page).ml(8.0).on_press_tx(
            move |d: &mut Dash, tx| {
                d.step_day(back, tx);
            },
        ));
    }
    bar = bar.child(el().grow(1.0));
    if s.shared.demo {
        bar = bar.child(
            text("Demo data")
                .key("demo-badge")
                .font(TextStyle::new(11.5, 600.0))
                .color(page.sub)
                .px(8.0)
                .py(3.0)
                .radius(6.0)
                .border(1.0, page.border)
                .mr(8.0),
        );
    }
    let trigger = icon_button("menu-button", &MORE, "Main menu", page).on_press(|d: &mut Dash| {
        d.overlay = if d.overlay == Overlay::Menu {
            Overlay::None
        } else {
            Overlay::Menu
        };
    });
    let sync = &s.settings.sync;
    let menu = Menu::new("main")
        .trigger(trigger)
        .open(s.overlay == Overlay::Menu)
        .placement(Placement::BottomEnd)
        .min_width(200.0)
        .on_close(|d: &mut Dash| d.overlay = Overlay::None)
        .item(
            MenuItem::new(if sync.running {
                "Syncing…"
            } else {
                "Sync Now"
            })
            .disabled(!sync.configured || sync.running)
            .on_select(|d: &mut Dash| {
                d.overlay = Overlay::None;
                sync::run(&d.shared, Trigger::Manual);
            }),
        )
        .item(
            MenuItem::new("Settings…")
                .shortcut(chord_label("mod+comma"))
                .on_select_tx(|d: &mut Dash, tx| settings::open(d, tx)),
        )
        .separator()
        .item(MenuItem::new("About Spent Day").on_select(|d: &mut Dash| d.overlay = Overlay::About))
        .item(
            MenuItem::new("Quit Spent Day")
                .shortcut(chord_label("mod+q"))
                .on_select_tx(|_: &mut Dash, tx| tx.quit()),
        )
        .build(k, layer);
    bar.child(menu)
}

fn chord_label(s: &str) -> String {
    Chord::parse(s).map(|c| c.label()).unwrap_or_default()
}

fn chip(key: &str, label: &str, page: &Page) -> El {
    text(label)
        .key(key)
        .role("button")
        .focusable(true)
        .font(TextStyle::new(12.5, 600.0))
        .color(page.text)
        .px(10.0)
        .py(4.0)
        .radius(999.0)
        .fill(page.track)
        .hover(page.hover)
}

fn banner(message: &str, page: &Page) -> El {
    let warn = oklch(0.80, 0.16, 75.0);
    row()
        .key("banner")
        .role("alert")
        .label(message.to_owned())
        .w_full()
        .mt(12.0)
        .px(14.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .radius(10.0)
        .fill(warn.alpha(0.12))
        .border(1.0, warn.alpha(0.35))
        .transition(Transition::opacity())
        .child(el().size(8.0, 8.0).radius(4.0).fill(warn))
        .child(
            text(message.to_owned())
                .font(TextStyle::new(13.0, 500.0))
                .color(page.text)
                .wrap()
                .grow(1.0),
        )
        .child(
            button("retry", "Try again", false, page).on_press_tx(|d: &mut Dash, tx| {
                d.refresh(tx);
            }),
        )
}

fn button(key: &str, label: &str, primary: bool, page: &Page) -> El {
    let e = text(label)
        .key(key)
        .role("button")
        .focusable(true)
        .font(TextStyle::new(13.0, 600.0))
        .px(14.0)
        .py(7.0)
        .radius(8.0)
        .press_scale(0.97);
    if primary {
        accent(e).color(Color::WHITE)
    } else {
        e.color(page.text).fill(page.track).hover(page.hover)
    }
}

/// What the headline, sentence and breadcrumbs read for the current drill.
struct Focus<'a> {
    total_ms: i64,
    sentence: String,
    crumbs: Vec<(&'static str, String, Option<Drill>)>,
    loaded: &'a Loaded,
}

fn focus<'a>(s: &Dash, l: &'a Loaded) -> Focus<'a> {
    let d = &l.detail;
    let tz = &s.shared.tz;
    let device = |id: &str| d.devices.iter().find(|v| v.device.id == id);
    let day_focus = |crumbs| Focus {
        total_ms: d.wall_ms,
        sentence: stats::day_sentence(d, l.usual_ms, day::weekday_name(s.selected), tz),
        crumbs,
        loaded: l,
    };
    match &s.drill {
        Drill::Day => day_focus(Vec::new()),
        Drill::Device(id) => match device(id) {
            Some(v) => Focus {
                total_ms: v.total_ms,
                sentence: stats::device_sentence(v, d.end_ms, tz),
                crumbs: vec![
                    ("crumb-day", "All devices".into(), Some(Drill::Day)),
                    ("crumb-device", v.device.name.clone(), None),
                ],
                loaded: l,
            },
            None => day_focus(Vec::new()),
        },
        Drill::Item { device: id, key } => {
            match device(id).and_then(|v| v.items.iter().find(|i| &i.key == key).map(|i| (v, i))) {
                Some((v, i)) => Focus {
                    total_ms: i.total_ms,
                    sentence: stats::item_sentence(i, &v.device.name, d.end_ms, tz),
                    crumbs: vec![
                        ("crumb-day", "All devices".into(), Some(Drill::Day)),
                        (
                            "crumb-device",
                            v.device.name.clone(),
                            Some(Drill::Device(id.clone())),
                        ),
                        ("crumb-item", i.name.clone(), None),
                    ],
                    loaded: l,
                },
                None => day_focus(Vec::new()),
            }
        }
    }
}

fn crumbs(list: Vec<(&'static str, String, Option<Drill>)>, page: &Page) -> El {
    let mut r = row()
        .key("crumbs")
        .role("navigation")
        .label("Breadcrumbs")
        .gap(4.0)
        .center_y()
        .mt(14.0)
        .ml(-6.0);
    let last = list.len().saturating_sub(1);
    for (i, (key, name, target)) in list.into_iter().enumerate() {
        let t = text(name.clone())
            .key(key)
            .font(TextStyle::new(13.0, 600.0))
            .px(6.0)
            .py(3.0)
            .radius(6.0);
        r = r.child(match target {
            Some(drill) => t
                .role("link")
                .label(name)
                .focusable(true)
                .color(page.sub)
                .hover(page.hover)
                .on_press_tx(move |d: &mut Dash, tx| d.drill_to(drill.clone(), tx)),
            None => t.color(page.text),
        });
        if i < last {
            r = r.child(icon(&NEXT, 11.0).color(page.faint));
        }
    }
    r
}

fn body(s: &Dash, l: &Loaded, layout: &MapLayout, map_w: f32, page: &Page, dark: bool) -> Vec<El> {
    let f = focus(s, l);
    let mut out = Vec::new();
    let drilled = !f.crumbs.is_empty();
    if drilled {
        out.push(crumbs(f.crumbs, page));
    }
    out.push(
        text(duration(f.total_ms))
            .key("headline")
            .role("heading")
            .label(format!(
                "{} of screen time",
                day::accessible_duration(f.total_ms)
            ))
            .font(TextStyle::new(46.0, 720.0))
            .color(page.text)
            .mt(if drilled { 6.0 } else { 22.0 }),
    );
    let empty = f.loaded.detail.wall_ms == 0;
    let mut line = row()
        .key("sentence-row")
        .gap(10.0)
        .center_y()
        .mt(2.0)
        .child(
            text(f.sentence)
                .key("sentence")
                .font(TextStyle::new(14.0, 450.0))
                .color(page.sub),
        );
    if s.drill == Drill::Day && !empty {
        line = line.child(
            row()
                .key("details-toggle")
                .role("button")
                .label("Details")
                .focusable(true)
                .expanded(s.details_open)
                .gap(3.0)
                .center_y()
                .px(6.0)
                .py(2.0)
                .radius(6.0)
                .hover(page.hover)
                .on_press(|d: &mut Dash| d.details_open = !d.details_open)
                .child(
                    icon(&NEXT, 12.0)
                        .color(page.text)
                        .rotate(if s.details_open { PI / 2.0 } else { 0.0 }),
                )
                .child(
                    text("Details")
                        .font(TextStyle::new(13.5, 600.0))
                        .color(page.text),
                ),
        );
    }
    out.push(line);
    if s.details_open && s.drill == Drill::Day && !empty {
        out.push(
            el().mt(12.0)
                .w_full()
                .child(details::details(l, &s.shared.tz, dark)),
        );
    }
    if empty {
        out.push(empty_state(page));
        return out;
    }
    out.push(
        el().key("map-area")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .mt(if drilled { 16.0 } else { 22.0 })
            .child(treemap::map(s, layout, dark)),
    );
    let hot = treemap::hot(s).and_then(|id| layout.cells.iter().find(|c| &c.id == id));
    out.push(el().mt(14.0).w_full().child(strip::strip(
        l,
        &s.drill,
        hot,
        map_w,
        &s.shared.tz,
        dark,
    )));
    out
}

fn empty_state(page: &Page) -> El {
    col()
        .key("empty")
        .role("group")
        .label("No screen time recorded")
        .w_full()
        .grow(1.0)
        .center()
        .gap(8.0)
        .child(
            el().size(64.0, 64.0)
                .radius(18.0)
                .center()
                .fill(page.track)
                .child(icon(&HOURGLASS, 30.0).color(page.sub)),
        )
        .child(
            text("No screen time recorded")
                .font(TextStyle::new(18.0, 650.0))
                .color(page.text)
                .mt(6.0),
        )
        .child(
            text("Nothing was recorded for this day.")
                .font(TextStyle::new(13.5, 450.0))
                .color(page.sub),
        )
}

fn loading(page: &Page) -> Vec<El> {
    let block = |key: &str, w: f32, h: f32| el().key(key).size(w, h).radius(8.0).fill(page.track);
    vec![
        block("loading-headline", 190.0, 44.0).mt(24.0),
        block("loading-sentence", 380.0, 14.0).mt(12.0),
        row()
            .key("loading")
            .role("progressbar")
            .label("Loading the day")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .mt(22.0)
            .gap(8.0)
            .child(el().grow(3.0).h_full().radius(10.0).fill(page.track))
            .child(
                col()
                    .grow(2.0)
                    .h_full()
                    .gap(8.0)
                    .child(el().w_full().grow(3.0).radius(10.0).fill(page.track))
                    .child(el().w_full().grow(2.0).radius(10.0).fill(page.track)),
            ),
    ]
}

/// A dimmed scrim with a centred card; a click outside or Escape closes it.
fn modal(key: &str, card: El, page: &Page) -> El {
    el().key(format!("{key}-scrim"))
        .w_full()
        .h_full()
        .absolute(0.0, 0.0)
        .center()
        .fill(Color::BLACK.alpha(0.4))
        .transition(Transition::opacity())
        .modal(true, |d: &mut Dash| d.overlay = Overlay::None)
        .child(
            card.key(key.to_owned())
                .role("dialog")
                .padding(28.0)
                .gap(10.0)
                .radius(16.0)
                .fill(page.surface)
                .border(1.0, page.border)
                .shadow(18.0, 48.0, Color::BLACK.alpha(0.35))
                .transition(Transition::opacity().combined(Transition::scale(0.96))),
        )
}

fn glyph() -> El {
    accent(el().size(56.0, 56.0).radius(14.0).center())
        .child(icon(&HOURGLASS, 28.0).color(Color::WHITE))
}

fn about(page: &Page) -> El {
    col()
        .label("About Spent Day")
        .w(380.0)
        .items_center()
        .child(glyph())
        .child(
            text("Spent Day")
                .font(TextStyle::new(22.0, 720.0))
                .color(page.text)
                .mt(6.0),
        )
        .child(
            text(format!("Version {}", env!("CARGO_PKG_VERSION")))
                .font(TextStyle::new(13.0, 500.0))
                .color(page.sub),
        )
        .child(
            text("Screen time across all your devices, synced end-to-end encrypted.")
                .font(TextStyle::new(13.5, 450.0))
                .color(page.text)
                .wrap()
                .align(rune::Align::Center)
                .mt(6.0),
        )
        .child(
            text("MIT License · The Spent Day contributors")
                .font(TextStyle::new(12.0, 450.0))
                .color(page.faint),
        )
        .child(
            button("about-close", "Close", true, page)
                .default_button()
                .mt(10.0)
                .on_press(|d: &mut Dash| d.overlay = Overlay::None),
        )
}

fn onboarding(page: &Page) -> El {
    let noun = platform::DEVICE_NOUN;
    let step = |title: &str, detail: String, hue: f32| {
        row()
            .gap(12.0)
            .items_start()
            .child(
                el().size(10.0, 10.0)
                    .radius(5.0)
                    .mt(5.0)
                    .fill(oklch(0.7, 0.17, hue)),
            )
            .child(
                col()
                    .grow(1.0)
                    .gap(2.0)
                    .child(
                        text(title.to_owned())
                            .font(TextStyle::new(14.0, 650.0))
                            .color(page.text),
                    )
                    .child(
                        text(detail)
                            .font(TextStyle::new(13.0, 450.0))
                            .color(page.sub)
                            .wrap(),
                    ),
            )
    };
    col()
        .label("Welcome to Spent Day")
        .w(480.0)
        .gap(14.0)
        .child(glyph())
        .child(text("Welcome to Spent Day").font(TextStyle::new(22.0, 720.0)).color(page.text))
        .child(
            text(format!(
                "Spent Day records which app and website are in front of you, on this {noun} and your other devices. It stores app names, website domains, times and this {noun}'s name. It never stores page addresses, page titles or window titles."
            ))
            .font(TextStyle::new(13.5, 450.0))
            .color(page.text)
            .wrap(),
        )
        .child(step(
            "Websites",
            format!("The system asks once per browser the first time you use Chrome or another Chromium browser on this {noun}. Safari sites are off until you turn them on in Settings. Firefox needs the Spent Day extension."),
            258.0,
        ))
        .child(step(
            "Sync",
            "End-to-end encrypted. Set it up later from Settings in the dashboard.".into(),
            330.0,
        ))
        .child(
            row().w_full().mt(6.0).child(el().grow(1.0)).child(
                button("onboarding-done", "Get Started", true, page)
                    .default_button()
                    .on_press(|d: &mut Dash| {
                        platform::write_pref(Pref::OnboardingDone, true);
                        d.overlay = Overlay::None;
                    }),
            ),
        )
}
