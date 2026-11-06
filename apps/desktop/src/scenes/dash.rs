//! Dashboard scenes, on the demo day (2026-10-28). Bodies until unit 2.

use crate::ui::dash::{Dash, Load, Overlay};
use jiff::civil::date;

fn demo_day(s: &mut Dash) {
    s.selected = date(2026, 10, 28);
}

pub fn day(s: &mut Dash) {
    demo_day(s);
}
pub fn hover(s: &mut Dash) {
    demo_day(s);
}
pub fn other_popover(s: &mut Dash) {
    demo_day(s);
}
pub fn device(s: &mut Dash) {
    demo_day(s);
}
pub fn sessions(s: &mut Dash) {
    demo_day(s);
}
pub fn empty(s: &mut Dash) {
    s.selected = date(2026, 10, 27);
}
pub fn loading(s: &mut Dash) {
    demo_day(s);
    s.load = Load::Loading;
}
pub fn error(s: &mut Dash) {
    demo_day(s);
    s.error = Some("Could not read or write the database".into());
}
pub fn details(s: &mut Dash) {
    demo_day(s);
    s.details_open = true;
}
pub fn focus(s: &mut Dash) {
    demo_day(s);
}
pub fn reduced(s: &mut Dash) {
    demo_day(s);
    s.force_reduced = true;
}
pub fn menu(s: &mut Dash) {
    demo_day(s);
    s.overlay = Overlay::Menu;
}
pub fn about(s: &mut Dash) {
    demo_day(s);
    s.overlay = Overlay::About;
}
pub fn onboarding(s: &mut Dash) {
    demo_day(s);
    s.overlay = Overlay::Onboarding;
}
