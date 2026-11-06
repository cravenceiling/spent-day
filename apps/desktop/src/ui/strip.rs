//! The day strip (full width and the popover's mini strip). Stub until unit 2.

use crate::ui::dash::Loaded;
use jiff::tz::TimeZone;
use rune::{El, el};

pub fn strip(loaded: &Loaded, hovered: Option<&str>, width: f32, tz: &TimeZone, dark: bool) -> El {
    let _ = (loaded, hovered, width, tz, dark);
    el().key("strip")
}
