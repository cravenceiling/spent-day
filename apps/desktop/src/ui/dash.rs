//! Dashboard state and its transitions. Bodies are stubs until unit 2.

use crate::app::Shared;
use crate::palette::Palette;
use crate::ui::settings::Prefs;
use jiff::civil::Date;
use rune::{KeyEvent, Tx};
use spentday_core::{DayDetail, DayQuery};
use std::sync::Arc;

/// `{device}/{key}`, `{device}/other`, `{device}/{key}/{i}`.
pub type CellId = String;

pub struct Dash {
    pub shared: Arc<Shared>,
    /// The day shown.
    pub selected: Date,
    /// Today at the last refresh, for the midnight rollover.
    pub observed_today: Date,
    pub load: Load,
    /// Banner text, kept until a load succeeds.
    pub error: Option<String>,
    /// Bumped per refresh; results of older ones are dropped.
    pub generation: u64,
    pub drill: Drill,
    /// Survives drill round trips, reset on a day change.
    pub details_open: bool,
    /// Immediate: dims the other tiles and the strip.
    pub hovered: Option<CellId>,
    /// After 300 ms; Other opens at once.
    pub popover: Option<CellId>,
    /// Hover epoch for the 90 ms leave grace.
    pub leave_due: Option<u64>,
    pub hover_epoch: u64,
    /// The tile key the next view marks `focus_selected(true)`.
    pub focus_after_drill: Option<String>,
    pub overlay: Overlay,
    /// The settings overlay's state, kept across opens.
    pub settings: Prefs,
    /// Scenes only: `cx.reduced_motion(true)`.
    pub force_reduced: bool,
}

pub enum Load {
    Loading,
    Ready(Arc<Loaded>),
}

pub struct Loaded {
    pub query: DayQuery,
    pub detail: DayDetail,
    /// Mean of positive totals, with at least 2 samples.
    pub usual_ms: Option<i64>,
    pub palette: Palette,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drill {
    Day,
    Device(String),
    Item { device: String, key: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    Menu,
    About,
    Onboarding,
    Settings,
}

impl Dash {
    pub fn new(shared: Arc<Shared>, selected: Date) -> Dash {
        let today = crate::day::today(&shared.tz);
        Dash {
            shared,
            selected,
            observed_today: today,
            load: Load::Loading,
            error: None,
            generation: 0,
            drill: Drill::Day,
            details_open: false,
            hovered: None,
            popover: None,
            leave_due: None,
            hover_epoch: 0,
            focus_after_drill: None,
            overlay: Overlay::None,
            settings: Prefs::new(),
            force_reduced: false,
        }
    }

    /// Rolls over at midnight, bumps `generation` and loads the selected day plus 4 usual days.
    pub fn refresh(&mut self, tx: &mut Tx) -> bool {
        let _ = tx;
        false
    }

    /// Drops a stale `generation`. Returns false when nothing visible changed.
    pub fn apply_load(
        &mut self,
        generation: u64,
        result: Result<Loaded, String>,
        tx: &mut Tx,
    ) -> bool {
        let _ = (generation, result, tx);
        false
    }

    /// One calendar day back or forward, never past today.
    pub fn step_day(&mut self, by: i32, tx: &mut Tx) -> bool {
        let _ = (by, tx);
        false
    }

    pub fn drill_to(&mut self, drill: Drill, tx: &mut Tx) {
        let _ = tx;
        self.drill = drill;
    }

    /// Escape: closes the popover, else pops one drill level. False at `Day`, so the key goes on.
    pub fn back(&mut self, tx: &mut Tx) -> bool {
        let _ = tx;
        false
    }

    pub fn hover_enter(&mut self, id: CellId, tx: &mut Tx) {
        let _ = (id, tx);
    }

    pub fn hover_leave(&mut self, id: CellId, tx: &mut Tx) {
        let _ = (id, tx);
    }

    pub fn popover_open(&mut self, id: CellId, tx: &mut Tx) {
        let _ = (id, tx);
    }

    /// The pointer entered (true) or left the popover card.
    pub fn popover_hold(&mut self, inside: bool, tx: &mut Tx) {
        let _ = (inside, tx);
    }

    /// Escape, mod+left/right, mod+comma, mod+w, mod+q, Return on the focused tile.
    pub fn on_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let _ = (ev, tx);
        false
    }
}
