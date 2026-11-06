//! The menu bar panel (macOS): today's total, the top 3 apps, Open Dashboard, Sync Now,
//! Settings…, Quit. Linux registers the window for headless scenes only. Stub until unit 3.

use crate::app::Shared;
use crate::sync::SyncView;
use crate::ui::{bg, font, kit};
use rune::{Cx, El, Key, KeyEvent, Tx, col, text};
use std::sync::Arc;

pub const SIZE: (f32, f32) = (264.0, 220.0);

pub struct Panel {
    pub shared: Arc<Shared>,
    /// Today's `wall_ms`; 0 until loaded.
    pub total_ms: i64,
    /// `Summary::apps` top 3, in core rank order.
    pub rows: Vec<PanelRow>,
    pub sync: SyncView,
    /// A new window reports focus lost once before it activates; dismiss only after it had focus.
    pub armed: bool,
    pub generation: u64,
}

pub struct PanelRow {
    pub name: String,
    pub app_ids: Vec<String>,
    pub total_ms: i64,
    pub icon: Option<rune::Image>,
}

impl Panel {
    pub fn new(shared: Arc<Shared>) -> Panel {
        Panel {
            shared,
            total_ms: 0,
            rows: Vec::new(),
            sync: SyncView::default(),
            armed: false,
            generation: 0,
        }
    }

    /// Loads today's summary off the event loop.
    pub fn refresh(&mut self, tx: &mut Tx) -> bool {
        let _ = tx;
        false
    }

    pub fn on_focus(&mut self, focused: bool, tx: &mut Tx) {
        if focused {
            self.armed = true;
        } else if self.armed {
            self.armed = false;
            tx.close_window();
        }
    }

    pub fn on_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if ev.key != Key::Escape {
            return false;
        }
        tx.close_window();
        true
    }
}

pub fn view(s: &Panel, cx: &mut Cx<Panel>) -> El {
    let dark = cx.system_dark();
    let k = kit(dark);
    col()
        .key("panel")
        .w_full()
        .h_full()
        .fill(bg(dark))
        .px(16.0)
        .py(14.0)
        .gap(8.0)
        .child(text("Today").font(font::LABEL).color(k.c.text_muted))
        .child(
            text(crate::format::duration(s.total_ms))
                .key("total")
                .font(font::HEADLINE)
                .color(k.c.text_base),
        )
}
