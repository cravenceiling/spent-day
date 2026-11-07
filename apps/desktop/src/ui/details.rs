//! The details panel under the sentence: overlap, longest stretch, late time.

use crate::stats;
use crate::ui::dash::Loaded;
use crate::ui::dash_view::Page;
use jiff::tz::TimeZone;
use rune::{El, ScrollSpec, TextStyle, Transition, col, el, text};

pub fn details(loaded: &Loaded, tz: &TimeZone, dark: bool) -> El {
    let page = Page::of(dark);
    let lines = col().key("details-lines").gap(6.0).children(
        stats::details_lines(&loaded.detail, tz)
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                text(line)
                    .key(format!("details-{i}"))
                    .font(TextStyle::new(13.0, 450.0))
                    .color(page.text)
                    .wrap()
            }),
    );
    el().key("details")
        .role("group")
        .label("Details")
        .w_full()
        .max_h(170.0)
        .min_h(0.0)
        .px(14.0)
        .py(12.0)
        .radius(10.0)
        .fill(page.panel)
        .border(1.0, page.border)
        .transition(Transition::opacity())
        .child(
            lines
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .shrink()
                .min_h(0.0),
        )
}
