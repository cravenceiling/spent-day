//! Direction B: jewel gradients, whole-day Oklab separation and uniform hover depth.

use crate::colors::{self, Lch, Rgb, cusp, fit, lch, max_chroma};
use crate::icons::Seeds;
use spentday_core::{DayDetail, ItemKind};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileStyle {
    pub base: Rgb,
    /// Top and bottom stops, for a vertical gradient at PI radians.
    pub gradient: [Rgb; 2],
    pub ink: Rgb,
    pub dim_fill: Rgb,
    pub dim_gradient: [Rgb; 2],
    pub dim_ink: Rgb,
}

impl TileStyle {
    pub fn from_base(base: Lch, dark: bool) -> Self {
        let [top, bottom] = stops(base);
        let (dim_fill, dim_ink) = dim(base, dark);
        Self {
            base: base.rgb(),
            gradient: [top.rgb(), bottom.rgb()],
            ink: base.rgb().ink(),
            dim_fill,
            dim_gradient: [dim(top, dark).0, dim(bottom, dark).0],
            dim_ink,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Palette {
    /// Per item key, `[dark, light]`. Normal stops match; dimmed stops and ink follow the page.
    pub styles: HashMap<String, [TileStyle; 2]>,
}

impl Palette {
    /// `on_seed` runs on a lookup worker when an icon colour this build lacked arrives.
    pub fn build(
        detail: &DayDetail,
        seeds: &Seeds,
        demo: bool,
        on_seed: impl Fn() + Clone + Send + 'static,
    ) -> Palette {
        let mut ranked = HashMap::new();
        for item in detail.devices.iter().flat_map(|d| &d.items) {
            let entry = ranked.entry(item.key.clone()).or_insert((0, item));
            entry.0 += item.total_ms;
        }
        let mut ranked: Vec<_> = ranked.into_iter().collect();
        ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
        let candidates: Vec<_> = ranked
            .into_iter()
            .map(|(key, (_, item))| {
                let seed = if demo {
                    crate::demo::color_seed(item)
                } else if item.kind == ItemKind::Site {
                    colors::hashed(&key)
                } else {
                    seeds
                        .get_or_spawn(&item.app_key, &item.app_ids, on_seed.clone())
                        .unwrap_or_else(|| colors::hashed(&key))
                };
                (key, candidates(seed))
            })
            // Other is synthesized by layout; precompute it so views never search its gamut.
            .chain(std::iter::once((
                "other".into(),
                candidates(colors::hashed("other")),
            )))
            .collect();
        Palette {
            styles: colors::separate(candidates)
                .into_iter()
                .map(|(key, base)| {
                    (
                        key,
                        [
                            TileStyle::from_base(base, true),
                            TileStyle::from_base(base, false),
                        ],
                    )
                })
                .collect(),
        }
    }

    pub fn style(&self, key: &str, dark: bool) -> TileStyle {
        self.styles.get(key).map_or_else(
            || TileStyle::from_base(candidates(colors::hashed(key))[0], dark),
            |styles| styles[usize::from(!dark)],
        )
    }

    pub fn fill(&self, key: &str, dark: bool) -> Rgb {
        self.style(key, dark).base
    }

    pub fn session_style(&self, key: &str, index: usize, dark: bool) -> TileStyle {
        TileStyle::from_base(session_fill(self.fill(key, dark), index).lch(), dark)
    }
}

pub fn page(dark: bool) -> Rgb {
    if dark {
        lch(0.17, 0.012, 285.)
    } else {
        lch(0.97, 0.006, 285.)
    }
    .rgb()
}

pub fn page_ink(dark: bool) -> Rgb {
    if dark {
        lch(0.96, 0.005, 285.)
    } else {
        lch(0.18, 0.01, 285.)
    }
    .rgb()
}

pub fn secondary_ink(dark: bool) -> Rgb {
    if dark {
        lch(0.72, 0.01, 285.)
    } else {
        lch(0.48, 0.01, 285.)
    }
    .rgb()
}

pub fn dark_shift(h: f64, amount: f64) -> f64 {
    if h.rem_euclid(360.) < 120. {
        -amount
    } else {
        amount
    }
}

pub fn light_shift(h: f64, amount: f64) -> f64 {
    if (200.0..300.0).contains(&h.rem_euclid(360.)) {
        0.
    } else {
        -dark_shift(h, amount)
    }
}

fn stops(base: Lch) -> [Lch; 2] {
    [
        fit(lch(
            base.l + 0.035,
            base.c,
            base.h + light_shift(base.h, 6.),
        )),
        fit(lch(base.l - 0.04, base.c, base.h + dark_shift(base.h, 8.))),
    ]
}

const ANCHORS: [f64; 12] = [
    22., 45., 78., 132., 158., 192., 232., 258., 280., 304., 330., 354.,
];

fn b_deep(h: f64) -> Option<Lch> {
    // Deep orange through olive reads as brown, so those hues always use the bright variant.
    if (35.0..115.0).contains(&h.rem_euclid(360.)) {
        return None;
    }
    let mut l = 0.60_f64.min(cusp(h).0);
    while l > 0.40 {
        let c = (max_chroma(l, h) * 0.96).min(0.26);
        let [top, bottom] = stops(lch(l, c, h));
        if top.rgb().contrast(Rgb::WHITE) >= 4.6 {
            return (c >= 0.16 && bottom.c >= 0.15).then_some(lch(l, c, h));
        }
        l -= 0.01;
    }
    None
}

fn b_bright(h: f64) -> Lch {
    let mut l = cusp(h).0.clamp(0.74, 0.82);
    loop {
        let c = (max_chroma(l, h) * 0.95).min(0.24);
        let bottom = stops(lch(l, c, h))[1];
        if bottom.rgb().contrast(Rgb::BLACK) >= 4.6 || l > 0.95 {
            return lch(l, c, h);
        }
        l += 0.01;
    }
}

pub fn candidates(seed: Lch) -> Vec<Lch> {
    if seed.c < 0.03 {
        return vec![
            lch(0.50, 0.08, 268.),
            lch(0.50, 0.075, 205.),
            lch(0.74, 0.07, 268.),
        ];
    }
    let h = seed.h.rem_euclid(360.);
    let distance = |a: f64| ((a - h + 540.) % 360. - 180.).abs();
    let i = (0..ANCHORS.len())
        .min_by(|&i, &j| distance(ANCHORS[i]).total_cmp(&distance(ANCHORS[j])))
        .unwrap();
    let toward = if ((h - ANCHORS[i] + 540.) % 360. - 180.) >= 0. {
        1
    } else {
        11
    };
    let order = [i, (i + toward) % 12, (i + 12 - toward) % 12];
    let primary = |k: usize| b_deep(ANCHORS[k]).unwrap_or_else(|| b_bright(ANCHORS[k]));
    let alt = |k: usize| b_deep(ANCHORS[k]).map(|_| b_bright(ANCHORS[k]));
    let mut out = vec![primary(order[0])];
    out.extend(alt(order[0]));
    out.extend([primary(order[1]), primary(order[2])]);
    out.extend(alt(order[1]));
    out.extend(alt(order[2]));
    out
}

/// Preserve chronological lightness variations while keeping both stops readable.
pub fn session_fill(base: Rgb, i: usize) -> Rgb {
    let mut color = base.lch();
    color.l = (color.l + [0., -0.06, 0.04, -0.1, -0.02][i % 5]).clamp(0.3, 0.9);
    let ink = base.ink();
    loop {
        if stops(color)
            .iter()
            .all(|stop| stop.rgb().contrast(ink) >= 4.6)
        {
            return color.rgb();
        }
        color.l += if ink == Rgb::WHITE { -0.01 } else { 0.01 };
    }
}

/// Non-hovered tiles keep their hue but switch to the page's ink at a shared depth.
pub fn dim(color: Lch, dark: bool) -> (Rgb, Rgb) {
    if dark {
        (
            lch(
                0.30 + (color.l - 0.6) * 0.08,
                (color.c * 0.35).min(0.06),
                color.h,
            )
            .rgb(),
            Rgb::WHITE,
        )
    } else {
        (
            lch(
                0.92 + (color.l - 0.6) * 0.05,
                (color.c * 0.30).min(0.045),
                color.h,
            )
            .rgb(),
            Rgb::BLACK,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{day, demo::Demo};
    use jiff::{civil::date, tz::TimeZone};

    fn hex(rgb: Rgb) -> u32 {
        let [r, g, b] = [rgb.r, rgb.g, rgb.b].map(|v| (v * 255.).round() as u32);
        r << 16 | g << 8 | b
    }

    #[test]
    fn exact_jewel_demo_stops_and_ink_in_both_themes() {
        let tz = TimeZone::UTC;
        let detail = Demo::with_timezone(tz.clone()).day(day::query(date(2026, 10, 28), &tz));
        let palette = Palette::build(&detail, &Seeds::default(), true, || {});
        for (key, top, bottom, ink) in [
            ("app:com.apple.dt.Xcode", 0x1d71e2, 0x2d53c9, Rgb::WHITE),
            ("app:instagram", 0xd02c92, 0xba005d, Rgb::WHITE),
            ("app:youtube", 0xde2d27, 0xbe0041, Rgb::WHITE),
            ("site:github.com", 0x596b9c, 0x4c5484, Rgb::WHITE),
            ("app:com.apple.MobileSMS", 0x87b8ff, 0x799bee, Rgb::BLACK),
            ("app:vscode", 0x47ebdc, 0x00d0d9, Rgb::BLACK),
            ("app:slack", 0xc030ce, 0xb30095, Rgb::WHITE),
            ("site:youtube.com", 0xff9386, 0xec7282, Rgb::BLACK),
            ("site:news.ycombinator.com", 0xfdc233, 0xf49f1d, Rgb::BLACK),
            ("app:com.apple.mail", 0x5b3cf9, 0x5c00d7, Rgb::WHITE),
            ("app:com.apple.Terminal", 0x2d7a82, 0x126371, Rgb::WHITE),
            ("app:chrome", 0xace709, 0x5ed740, Rgb::BLACK),
        ] {
            for dark in [true, false] {
                let style = palette.style(key, dark);
                assert_eq!(style.gradient.map(hex), [top, bottom], "{key}, dark={dark}");
                assert_eq!(style.ink, ink);
                assert_eq!(style.dim_ink, if dark { Rgb::WHITE } else { Rgb::BLACK });
                for stop in style.gradient {
                    assert!(stop.contrast(style.ink) >= 4.6, "{key}");
                }
                for stop in style.dim_gradient {
                    assert!(stop.contrast(style.dim_ink) >= 10.9, "{key}");
                }
            }
        }
        assert_eq!(palette.styles.len(), 13);
        let github = palette.style("site:github.com", true).base.lch();
        let terminal = palette.style("app:com.apple.Terminal", true).base.lch();
        assert!((colors::ok_distance(github, terminal) - 0.0811).abs() < 0.0001);
    }

    #[test]
    fn palette_is_stable_across_device_and_item_order() {
        let tz = TimeZone::UTC;
        let mut detail = Demo::with_timezone(tz.clone()).day(day::query(date(2026, 10, 28), &tz));
        let seeds = Seeds::default();
        let palette = Palette::build(&detail, &seeds, true, || {});
        detail.devices.reverse();
        for device in &mut detail.devices {
            device.items.reverse();
        }
        assert_eq!(palette, Palette::build(&detail, &seeds, true, || {}));
    }

    #[test]
    fn cached_app_seed_replaces_hash_and_sites_keep_hash() {
        let tz = TimeZone::UTC;
        let detail = Demo::with_timezone(tz.clone()).day(day::query(date(2026, 10, 28), &tz));
        let seeds = Seeds::default();
        let red = Rgb::hex(0xff0000).lch();
        for item in detail
            .devices
            .iter()
            .flat_map(|d| &d.items)
            .filter(|i| i.kind == ItemKind::App)
        {
            seeds
                .0
                .lock()
                .unwrap()
                .insert(item.app_key.clone(), Some(red));
        }
        let palette = Palette::build(&detail, &seeds, false, || {});
        assert_eq!(
            palette.style("app:com.apple.dt.Xcode", true),
            TileStyle::from_base(candidates(red)[0], true)
        );
        assert!(
            seeds
                .0
                .lock()
                .unwrap()
                .keys()
                .all(|k| !k.starts_with("site:"))
        );
    }

    #[test]
    fn literal_shifts_dim_and_page_colors() {
        assert_eq!(dark_shift(78., 8.), -8.);
        assert_eq!(dark_shift(258., 8.), 8.);
        assert_eq!(light_shift(258., 6.), 0.);
        assert_eq!(light_shift(78., 6.), 6.);
        assert_eq!(light_shift(330., 6.), -6.);
        assert!(b_deep(45.).is_none() && b_deep(78.).is_none());
        assert_eq!(candidates(lch(0.8, 0.029, 120.))[0], lch(0.50, 0.08, 268.));
        let (dark, dark_ink) = dim(lch(0.6, 0.2, 258.), true);
        let (light, light_ink) = dim(lch(0.6, 0.2, 258.), false);
        let expected_dark = lch(0.30, 0.06, 258.).rgb();
        let expected_light = lch(0.92, 0.045, 258.).rgb();
        assert_eq!(dark, expected_dark);
        assert_eq!(light, expected_light);
        assert_eq!((dark_ink, light_ink), (Rgb::WHITE, Rgb::BLACK));
        for dark in [true, false] {
            assert!(page(dark).contrast(page_ink(dark)) >= 14.);
            assert!(page(dark).contrast(secondary_ink(dark)) >= 4.5);
        }
    }

    #[test]
    fn all_labels_pass_at_rest_and_dimmed_including_session_variants() {
        for hue in (0..360).step_by(3) {
            for candidate in candidates(lch(0.6, 0.2, f64::from(hue))) {
                for dark in [true, false] {
                    for index in 0..5 {
                        let base = session_fill(candidate.rgb(), index).lch();
                        let style = TileStyle::from_base(base, dark);
                        for stop in style.gradient {
                            assert!(
                                stop.contrast(style.ink) >= 4.6 - 1e-6,
                                "hue {hue}, session {index}, {base:?}"
                            );
                        }
                        for stop in style.dim_gradient {
                            assert!(
                                stop.contrast(style.dim_ink) >= 10.9,
                                "hue {hue}, dark {dark}, session {index}"
                            );
                        }
                    }
                }
            }
        }
    }
}
