//! Settings overlay, panel and fatal window scenes. Bodies until unit 3.

use crate::ui::dash::{Dash, Overlay};
use crate::ui::fatal::Fatal;
use crate::ui::panel::Panel;

fn open(s: &mut Dash) {
    s.overlay = Overlay::Settings;
}

pub fn off(s: &mut Dash) {
    open(s);
}
pub fn off_invalid(s: &mut Dash) {
    open(s);
}
pub fn joining(s: &mut Dash) {
    open(s);
}
pub fn on(s: &mut Dash) {
    open(s);
}
pub fn on_revealed(s: &mut Dash) {
    open(s);
}
pub fn running(s: &mut Dash) {
    open(s);
}
pub fn failed(s: &mut Dash) {
    open(s);
}
pub fn skipped(s: &mut Dash) {
    open(s);
}
pub fn general_mac(s: &mut Dash) {
    open(s);
}

pub fn panel(s: &mut Panel) {
    s.total_ms = 49_800_000;
}
pub fn panel_syncing(s: &mut Panel) {
    s.total_ms = 49_800_000;
    s.sync.running = true;
}

pub fn db_error(s: &mut Fatal) {
    s.title = "Cannot open the database".into();
    s.body = "Could not read or write the database".into();
}
pub fn track_error(s: &mut Fatal) {
    s.title = "Spent Day can't track apps".into();
    s.body = "No supported desktop session was found.".into();
}
