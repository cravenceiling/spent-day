//! `MapLayout` to elements: device headers, tiles, the focus group, hover and the popover card.
//! Stub until unit 2.

use crate::ui::cells::MapLayout;
use crate::ui::dash::Dash;
use rune::{El, el};

pub fn map(s: &Dash, layout: &MapLayout, dark: bool) -> El {
    let _ = (s, layout, dark);
    el().key("map")
}

/// The anchored sessions card for `s.popover`, mounted at the root.
pub fn popover(s: &Dash, layout: &MapLayout, dark: bool) -> Option<El> {
    let _ = (s, layout, dark);
    None
}
