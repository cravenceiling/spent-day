//! Deterministic settings, panel and startup failure fixtures.

use crate::sync::{LastSync, SyncView};
use crate::ui::dash::{Dash, Overlay};
use crate::ui::fatal::Fatal;
use crate::ui::panel::{Panel, PanelRow};
use crate::ui::settings::{Autostart, Prefs};
use jiff::Timestamp;
use rune::TextEdit;
use spentday_core::{SyncConfig, SyncReport};

pub const CODE: &str = "7KQ2-M3XP-R2D9-WF4H-B8TN-C6JV-Q1ZE-HY5G-4MRA-T0VD-9XKP-E3NW-J7FB-2QHS";

fn open(s: &mut Dash) {
    s.overlay = Overlay::Settings;
    s.settings = Prefs::new();
    s.settings.autostart = None;
    s.settings.safari = None;
    s.settings.config = Some(None);
    s.settings.server = TextEdit::new("https://sync.example.com");
}

pub fn off(s: &mut Dash) {
    open(s);
    s.settings.server = TextEdit::new("");
}
pub fn off_invalid(s: &mut Dash) {
    open(s);
    s.settings.server = TextEdit::new("ftp://x");
}
pub fn joining(s: &mut Dash) {
    open(s);
    s.settings.join = Some(TextEdit::new("7KQ2-M3XP-R2D9-WF4H"));
}
pub fn on(s: &mut Dash) {
    open(s);
    s.settings.apply_config(Some(SyncConfig {
        server_url: "https://sync.example.com".into(),
        sync_code: CODE.into(),
    }));
    s.settings.sync = SyncView {
        running: false,
        configured: true,
        last: Some(LastSync::Ok {
            at: Timestamp::now(),
            report: SyncReport {
                pushed_spans: 18,
                pulled_spans: 342,
                skipped_batches: 0,
                finished_at_ms: Timestamp::now().as_millisecond(),
            },
        }),
    };
}
pub fn on_revealed(s: &mut Dash) {
    on(s);
    s.settings.reveal_code = true;
}
pub fn running(s: &mut Dash) {
    on(s);
    s.settings.sync.running = true;
}
pub fn failed(s: &mut Dash) {
    on(s);
    s.settings.sync.last = Some(LastSync::Failed { at: Timestamp::now(),
        message: "Couldn't reach the sync server or read its answer. Check the address and your connection.".into() });
}
pub fn skipped(s: &mut Dash) {
    on(s);
    if let Some(LastSync::Ok { report, .. }) = &mut s.settings.sync.last {
        report.skipped_batches = 1;
    }
}
pub fn general_mac(s: &mut Dash) {
    on(s);
    s.settings.autostart = Some(Autostart {
        on: true,
        note: None,
    });
    s.settings.safari = Some(false);
}

pub fn panel(s: &mut Panel) {
    s.total_ms = 49_800_000;
    s.sync.configured = true;
    s.rows = [
        ("Xcode", "com.apple.dt.Xcode", 21_900_000),
        ("Google Chrome", "com.google.Chrome", 7_200_000),
        ("Terminal", "com.apple.Terminal", 3_600_000),
    ]
    .into_iter()
    .map(|(name, id, total_ms)| PanelRow {
        name: name.into(),
        app_ids: vec![id.into()],
        total_ms,
        icon: None,
    })
    .collect();
}
pub fn panel_syncing(s: &mut Panel) {
    panel(s);
    s.sync.running = true;
}

pub fn db_error(s: &mut Fatal) {
    s.title = "Cannot open the database".into();
    s.body = "Spent Day couldn't read its database. The database file could not be opened.".into();
}
pub fn track_error(s: &mut Fatal) {
    s.title = "Spent Day can't track apps".into();
    s.body = "No supported desktop session was found.".into();
}
