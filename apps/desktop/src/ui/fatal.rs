//! The startup error window. Quit exits with code 3 (`spentday.service` does not restart it).

use crate::ui::{bg, font, kit};
use rune::{Cx, El, col, text};
use rune_kit::Button;

pub const SIZE: (f32, f32) = (440.0, 200.0);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fatal {
    /// "Cannot open the database" or "Spent Day can't track apps".
    pub title: String,
    pub body: String,
}

pub fn view(s: &Fatal, cx: &mut Cx<Fatal>) -> El {
    let dark = cx.system_dark();
    let k = kit(dark);
    col()
        .key("fatal")
        .w_full()
        .h_full()
        .fill(bg(dark))
        .padding(20.0)
        .gap(10.0)
        .child(
            text(&s.title)
                .key("fatal-title")
                .font(font::TITLE)
                .color(k.c.text_base),
        )
        .child(
            text(&s.body)
                .key("fatal-body")
                .font(font::BODY)
                .color(k.c.text_muted),
        )
        .child(
            Button::new("Quit")
                .on_press_tx(|_: &mut Fatal, tx| tx.quit())
                .build(k)
                .key("fatal-quit")
                .default_button(),
        )
}
