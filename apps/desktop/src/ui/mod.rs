//! Shared look: one rune-kit `Kit` per appearance, the window background and the type scale.

pub mod cells;
pub mod dash;
pub mod dash_view;
pub mod details;
pub mod fatal;
pub mod panel;
pub mod settings;
pub mod strip;
pub mod treemap;

use rune::{Color, TextStyle};
use rune_kit::{Kit, PaletteSeed};
use std::sync::OnceLock;

pub fn bg(dark: bool) -> Color {
    if dark {
        rune::color::hex("#161618").unwrap()
    } else {
        rune::color::hex("#f7f7f9").unwrap()
    }
}

/// Built once per appearance: `Kit::from_palette` does OKLab work a view must not repeat.
pub fn kit(dark: bool) -> &'static Kit {
    static DARK: OnceLock<Kit> = OnceLock::new();
    static LIGHT: OnceLock<Kit> = OnceLock::new();
    let cell = if dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| {
        let fg = if dark {
            rune::color::hex("#f2f2f5").unwrap()
        } else {
            rune::color::hex("#1c1c1f").unwrap()
        };
        Kit::from_palette(PaletteSeed {
            bg: bg(dark),
            fg,
            accent: rune::tok::accent(0, dark),
        })
    })
}

pub mod font {
    use super::TextStyle;

    pub const HEADLINE: TextStyle = TextStyle::new(34.0, 650.0);
    pub const TITLE: TextStyle = TextStyle::new(17.0, 600.0);
    pub const BODY: TextStyle = TextStyle::new(13.0, 400.0);
    pub const LABEL: TextStyle = TextStyle::new(12.0, 500.0);
}
