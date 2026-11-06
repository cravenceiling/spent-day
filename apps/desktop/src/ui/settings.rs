//! The settings overlay inside the dashboard window: Sync, plus General rows where the platform
//! has them. Handlers are closures on `&mut Dash` that touch `d.settings`. Stub until unit 3.

use crate::platform::{self, Pref};
use crate::sync::SyncView;
use crate::ui::dash::{Dash, Overlay};
use crate::ui::font;
use rune::{Color, El, TextEdit, col, el, text};
use rune_kit::Kit;
use spentday_core::SyncConfig;
use std::time::Instant;

pub struct Prefs {
    /// `None` while the first load runs.
    pub config: Option<Option<SyncConfig>>,
    pub server: TextEdit,
    /// `Some` while joining with a code.
    pub join: Option<TextEdit>,
    pub working: bool,
    /// Inline red text.
    pub error: Option<String>,
    pub reveal_code: bool,
    /// "Copied" shows until then; a `wake_after` clears it.
    pub copied_until: Option<Instant>,
    /// The Turn Off confirmation.
    pub confirm_off: bool,
    pub sync: SyncView,
    /// `None`: the platform has no such toggle and the row is hidden.
    pub autostart: Option<Autostart>,
    pub safari: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Autostart {
    pub on: bool,
    /// "Allow Spent Day in System Settings…" after SMAppService asks for approval.
    pub note: Option<String>,
}

impl Prefs {
    pub fn new() -> Prefs {
        Prefs {
            config: None,
            server: TextEdit::new(""),
            join: None,
            working: false,
            error: None,
            reveal_code: false,
            copied_until: None,
            confirm_off: false,
            sync: SyncView::default(),
            autostart: platform::autostart().map(|on| Autostart { on, note: None }),
            safari: platform::read_pref(Pref::RecordSafariSites),
        }
    }
}

/// The modal card, mounted at the dashboard's root.
pub fn view(d: &Dash, k: &Kit) -> El {
    let _ = &d.settings;
    el().key("settings-scrim")
        .w_full()
        .h_full()
        .absolute(0.0, 0.0)
        .center()
        .fill(Color::BLACK.alpha(0.35))
        .modal(true, |d: &mut Dash| d.overlay = Overlay::None)
        .child(
            col()
                .key("settings")
                .role("dialog")
                .label("Settings")
                .w(500.0)
                .padding(20.0)
                .radius(12.0)
                .fill(k.c.layer_01)
                .child(text("Settings").font(font::TITLE).color(k.c.text_base)),
        )
}
