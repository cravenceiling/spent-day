//! Dashboard scenes, on the demo day (2026-10-28). Each loads in place: a headless render has no
//! event loop to deliver a background load.

use crate::palette::Palette;
use crate::ui::dash::{Dash, Drill, Load, Loaded, Overlay};
use jiff::civil::date;
use spentday_core::{DayItem, ItemKind, Session};
use std::sync::Arc;

const MAC: &str = "demo-mac";
const XCODE: &str = "demo-mac/app:com.apple.dt.Xcode";

fn demo_day(s: &mut Dash) {
    s.selected = date(2026, 10, 28);
    s.load_now();
}

pub fn day(s: &mut Dash) {
    demo_day(s);
}
pub fn hover(s: &mut Dash) {
    demo_day(s);
    s.hovered = Some(XCODE.into());
    s.popover = Some(XCODE.into());
}
/// Two short apps on the MacBook fold into Other, and its card lists them.
pub fn other_popover(s: &mut Dash) {
    demo_day(s);
    let Load::Ready(l) = &s.load else { return };
    let mut detail = l.detail.clone();
    let start = detail.start_ms + 17 * 3_600_000;
    let mac = detail
        .devices
        .iter_mut()
        .find(|d| d.device.id == MAC)
        .unwrap();
    for (i, (name, minutes)) in [("Calculator", 1), ("Preview", 2)].into_iter().enumerate() {
        let start_ms = start + i as i64 * 600_000;
        let end_ms = start_ms + minutes * 60_000;
        mac.items.push(DayItem {
            key: format!("app:{name}"),
            name: name.into(),
            kind: ItemKind::App,
            app_key: name.into(),
            app_ids: vec![],
            total_ms: end_ms - start_ms,
            sessions: vec![Session { start_ms, end_ms }],
        });
        mac.total_ms += end_ms - start_ms;
        detail.device_ms += end_ms - start_ms;
        detail.wall_ms += end_ms - start_ms;
    }
    s.load = Load::Ready(Arc::new(Loaded {
        query: l.query.clone(),
        usual_ms: l.usual_ms,
        palette: Palette::build(&detail, &s.shared.icons, true, || {}),
        detail,
    }));
    let other = format!("{MAC}/other");
    s.hovered = Some(other.clone());
    s.popover = Some(other);
}
pub fn device(s: &mut Dash) {
    demo_day(s);
    s.drill = Drill::Device(MAC.into());
}
pub fn sessions(s: &mut Dash) {
    demo_day(s);
    s.drill = Drill::Item {
        device: MAC.into(),
        key: "app:com.apple.dt.Xcode".into(),
    };
}
pub fn empty(s: &mut Dash) {
    s.selected = date(2026, 10, 27);
    s.load_now();
}
pub fn loading(s: &mut Dash) {
    s.selected = date(2026, 10, 28);
    s.load = Load::Loading;
}
pub fn error(s: &mut Dash) {
    demo_day(s);
    s.error = Some(crate::format::user_message(
        &spentday_core::SpentDayError::Storage("The database file could not be opened.".into()),
    ));
}
pub fn details(s: &mut Dash) {
    demo_day(s);
    s.details_open = true;
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
