//! The details panel under the sentence. Stub until unit 2.

use crate::ui::dash::Loaded;
use jiff::tz::TimeZone;
use rune::{El, el};

pub fn details(loaded: &Loaded, tz: &TimeZone, dark: bool) -> El {
    let _ = (loaded, tz, dark);
    el().key("details")
}
