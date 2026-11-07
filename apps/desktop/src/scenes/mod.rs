//! Every headless scene, per window. Names are fixed here; bodies live with their owners:
//! `dash.rs` (dashboard), `settings.rs` (settings overlay, panel, fatal).

mod dash;
mod settings;

use crate::ui::dash::Dash;
use crate::ui::fatal::Fatal;
use crate::ui::panel::Panel;

pub type Scenes<S> = &'static [(&'static str, fn(&mut S))];

pub const DASH: Scenes<Dash> = &[
    ("day", dash::day),
    ("hover", dash::hover),
    ("other-popover", dash::other_popover),
    ("device", dash::device),
    ("sessions", dash::sessions),
    ("empty", dash::empty),
    ("loading", dash::loading),
    ("error", dash::error),
    ("details", dash::details),
    ("reduced", dash::reduced),
    ("menu", dash::menu),
    ("about", dash::about),
    ("onboarding", dash::onboarding),
    ("settings-off", settings::off),
    ("settings-off-invalid", settings::off_invalid),
    ("settings-joining", settings::joining),
    ("settings-on", settings::on),
    ("settings-on-revealed", settings::on_revealed),
    ("settings-running", settings::running),
    ("settings-failed", settings::failed),
    ("settings-skipped", settings::skipped),
    ("settings-general-mac", settings::general_mac),
];

pub const PANEL: Scenes<Panel> = &[
    ("panel", settings::panel),
    ("panel-syncing", settings::panel_syncing),
];

pub const FATAL: Scenes<Fatal> = &[
    ("db-error", settings::db_error),
    ("track-error", settings::track_error),
];
