//! Tile colours: keys ranked across devices, seeded (demo samples, app icons, hashes), kept
//! apart per theme. Stub until unit 1.

use crate::colors::Rgb;
use crate::icons::Seeds;
use spentday_core::DayDetail;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Palette {
    /// Per `DayItem::key`: `[dark, light]`.
    pub fills: HashMap<String, [Rgb; 2]>,
}

impl Palette {
    pub fn build(detail: &DayDetail, seeds: &Seeds, demo: bool) -> Palette {
        let _ = (detail, seeds, demo);
        Palette::default()
    }

    pub fn fill(&self, key: &str, dark: bool) -> Rgb {
        self.fills
            .get(key)
            .map_or(Rgb::hex(0x808080), |f| f[usize::from(!dark)])
    }
}

/// The `i`th session slice of an item drawn in `base`.
pub fn session_fill(base: Rgb, i: usize) -> Rgb {
    let _ = i;
    base
}

/// `fill` faded toward the background while another tile is hovered, keeping `ink` legible.
pub fn dim(fill: Rgb, bg: Rgb, ink: Rgb) -> Rgb {
    let _ = (bg, ink);
    fill
}
