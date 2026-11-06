//! The dashboard view: header, day navigation, headline, sentence, breadcrumbs, banners, the
//! treemap and the overlays. Placeholder until unit 2.

use crate::ui::dash::{Dash, Overlay};
use crate::ui::{bg, font, kit, settings};
use rune::{Cx, El, col, el, text};

pub fn view(s: &Dash, cx: &mut Cx<Dash>) -> El {
    let dark = cx.system_dark();
    let k = kit(dark);
    cx.reduced_motion(s.force_reduced || cx.system_reduced_motion());
    let base = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(bg(dark))
        .px(24.0)
        .py(20.0)
        .gap(12.0)
        .child(
            text("Spent Day")
                .key("title")
                .font(font::TITLE)
                .color(k.c.text_base),
        )
        .child(
            text(s.selected.to_string())
                .key("date")
                .font(font::BODY)
                .color(k.c.text_muted),
        )
        .child(
            el().key("map")
                .role("group")
                .label("Treemap")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .focus_group(),
        );
    let mut overlays = Vec::new();
    if s.overlay == Overlay::Settings {
        overlays.push(settings::view(s, k));
    }
    el().w_full().h_full().child(base).children(overlays)
}
