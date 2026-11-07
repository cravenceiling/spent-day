//! Dashboard state and its transitions. The view (`dash_view.rs`) only reads this.

use crate::app::Shared;
use crate::palette::Palette;
use crate::ui::settings::Prefs;
use crate::{day, format, jobs};
use jiff::Timestamp;
use jiff::civil::Date;
use rune::{Chord, Key, KeyEvent, Spring, Tx};
use spentday_core::{DayDetail, DayQuery};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
#[path = "dash_tests.rs"]
mod tests;

/// `{device}/{key}`, `{device}/other`, `{device}/{key}/{i}`.
pub type CellId = String;

/// The commit spring for drills: quick, a touch of overshoot.
pub const DRILL: Spring = Spring::new(0.42, 0.12);

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
    /// After 300 ms; Other and a switch while one is open come at once.
    pub popover: Option<CellId>,
    /// The pointer is on the popover card, so leaving its tile keeps it.
    pub popover_held: bool,
    /// The key the next page marks `focus_selected`, instead of its first tile: the tile or
    /// header a pop came back from.
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
        let today = day::today(&shared.tz);
        let onboarding = !shared.demo
            && crate::platform::read_pref(crate::platform::Pref::OnboardingDone) == Some(false);
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
            popover_held: false,
            focus_after_drill: None,
            overlay: if onboarding {
                Overlay::Onboarding
            } else {
                Overlay::None
            },
            settings: Prefs::new(),
            force_reduced: false,
        }
    }

    pub fn loaded(&self) -> Option<&Loaded> {
        match &self.load {
            Load::Ready(l) => Some(l),
            Load::Loading => None,
        }
    }

    /// Reads `selected` and its 4 usual days. Blocking: callers run it off the event loop.
    pub fn read(shared: &Shared, selected: Date) -> Result<Loaded, String> {
        let tz = &shared.tz;
        let read = |q: DayQuery| {
            shared
                .store
                .day(q)
                .map_err(|e| format::error("read the day", &e))
        };
        let query = day::query(selected, tz);
        let detail = read(query.clone())?;
        let history = day::usual_queries(selected, Timestamp::now(), tz)
            .into_iter()
            .map(read)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Loaded {
            usual_ms: day::usual_total(&history),
            palette: palette(shared, &detail),
            query,
            detail,
        })
    }

    /// Rebuilds the palette once an icon's colour arrives. False when nothing changed.
    pub fn repaint(&mut self) -> bool {
        let Load::Ready(current) = &self.load else {
            return false;
        };
        let palette = palette(&self.shared, &current.detail);
        if palette == current.palette {
            return false;
        }
        self.load = Load::Ready(Arc::new(Loaded {
            query: current.query.clone(),
            detail: current.detail.clone(),
            usual_ms: current.usual_ms,
            palette,
        }));
        true
    }

    /// Scenes: load on the spot, with no event loop to deliver a background result.
    pub fn load_now(&mut self) {
        match Dash::read(&self.shared, self.selected) {
            Ok(l) => self.load = Load::Ready(Arc::new(l)),
            Err(e) => self.error = Some(e),
        }
    }

    /// Rolls over at midnight, bumps `generation` and loads the selected day plus 4 usual days.
    pub fn refresh(&mut self, tx: &mut Tx) -> bool {
        let today = day::today(&self.shared.tz);
        let mut changed = false;
        if today != self.observed_today {
            if self.selected == self.observed_today {
                self.selected = today;
                self.reset_day();
                changed = true;
            }
            self.observed_today = today;
        }
        self.generation += 1;
        let generation = self.generation;
        let Some(ui) = self.shared.ui.get() else {
            // No windows (tests): load in place.
            let result = Dash::read(&self.shared, self.selected);
            return self.apply_load(generation, result, tx) || changed;
        };
        let (shared, selected) = (self.shared.clone(), self.selected);
        jobs::spawn(
            ui.dash.clone(),
            move || Dash::read(&shared, selected),
            move |d: &mut Dash, result, tx| d.apply_load(generation, result, tx),
        );
        changed
    }

    /// Drops a stale `generation`. Returns false when nothing visible changed.
    pub fn apply_load(
        &mut self,
        generation: u64,
        result: Result<Loaded, String>,
        tx: &mut Tx,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        let loaded = match result {
            Err(e) => {
                let changed = self.error.as_ref() != Some(&e);
                self.error = Some(e);
                return changed;
            }
            Ok(l) => l,
        };
        let had_error = self.error.take().is_some();
        if let Load::Ready(current) = &self.load
            && current.query == loaded.query
            && current.detail == loaded.detail
            && current.usual_ms == loaded.usual_ms
            && current.palette == loaded.palette
        {
            return had_error;
        }
        let detail = &loaded.detail;
        let device = |id: &str| detail.devices.iter().find(|d| d.device.id == id);
        self.drill = match std::mem::replace(&mut self.drill, Drill::Day) {
            Drill::Device(d) if device(&d).is_some() => Drill::Device(d),
            Drill::Item { device: d, key } => match device(&d) {
                Some(v) if v.items.iter().any(|i| i.key == key) => Drill::Item { device: d, key },
                Some(_) => Drill::Device(d),
                None => Drill::Day,
            },
            _ => Drill::Day,
        };
        let exists = |id: &CellId| cell_exists(detail, id);
        if !self.hovered.as_ref().is_none_or(exists) {
            self.hovered = None;
        }
        if !self.popover.as_ref().is_none_or(exists) {
            self.popover = None;
            self.popover_held = false;
        }
        if matches!(self.load, Load::Loading) {
            // The first data of a day appears in place rather than flying in.
            tx.instant();
        }
        self.load = Load::Ready(Arc::new(loaded));
        true
    }

    fn reset_day(&mut self) {
        self.drill = Drill::Day;
        self.hovered = None;
        self.popover = None;
        self.popover_held = false;
        self.focus_after_drill = None;
        self.details_open = false;
    }

    /// One calendar day back or forward, never past today.
    pub fn step_day(&mut self, by: i32, tx: &mut Tx) -> bool {
        let next = day::step(self.selected, by);
        if next > day::today(&self.shared.tz) || next == self.selected {
            return false;
        }
        self.selected = next;
        self.reset_day();
        self.load = Load::Loading;
        self.refresh(tx);
        true
    }

    pub fn drill_to(&mut self, drill: Drill, tx: &mut Tx) {
        if drill == self.drill {
            return;
        }
        self.hovered = None;
        self.popover = None;
        self.popover_held = false;
        self.focus_after_drill = None;
        // Rune snaps spatial travel itself under reduced motion.
        tx.motion(DRILL);
        self.drill = drill;
    }

    /// Escape: closes an overlay or the popover, else pops one drill level. False at `Day`, so
    /// the key goes on.
    pub fn back(&mut self, tx: &mut Tx) -> bool {
        if self.overlay != Overlay::None {
            self.overlay = Overlay::None;
            return true;
        }
        if self.popover.take().is_some() {
            self.popover_held = false;
            return true;
        }
        let (drill, focus) = match &self.drill {
            Drill::Day => return false,
            Drill::Device(d) => (Drill::Day, format!("hdr-{d}")),
            Drill::Item { device, key } => (
                Drill::Device(device.clone()),
                format!("tile-{device}/{key}"),
            ),
        };
        self.drill_to(drill, tx);
        self.focus_after_drill = Some(focus);
        true
    }

    /// The pointer entered a tile: dim the rest at once; an open popover follows it.
    pub fn hover_enter(&mut self, id: CellId) {
        if self.popover.is_some() {
            self.popover = Some(id.clone());
        }
        self.hovered = Some(id);
    }

    /// The pointer left a tile 90 ms ago and didn't come back.
    pub fn hover_leave(&mut self, id: CellId, tx: &mut Tx) {
        let _ = tx;
        if self.hovered.as_ref() == Some(&id) {
            self.hovered = None;
        }
        if self.popover.as_ref() == Some(&id) && !self.popover_held {
            self.popover = None;
        }
    }

    /// The pointer rested 300 ms on a tile (at once for Other), or the tile was pressed.
    pub fn popover_open(&mut self, id: CellId, tx: &mut Tx) {
        let _ = tx;
        self.hovered = Some(id.clone());
        self.popover = Some(id);
    }

    /// The pointer entered (true) or left the popover card.
    pub fn popover_hold(&mut self, inside: bool, tx: &mut Tx) {
        let _ = tx;
        self.popover_held = inside;
        if !inside && self.hovered != self.popover {
            self.popover = None;
        }
    }

    /// Escape, mod+left/right, mod+comma, mod+w, mod+q. Return on a tile is the map's own key.
    pub fn on_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if ev.key == Key::Escape {
            return self.back(tx);
        }
        let chord = |s: &str| Chord::parse(s).is_ok_and(|c| c.matches_tx(ev, tx));
        if chord("mod+left") {
            self.step_day(-1, tx);
        } else if chord("mod+right") {
            self.step_day(1, tx);
        } else if chord("mod+comma") {
            crate::ui::settings::open(self, tx);
        } else if chord("mod+w") {
            tx.close_window();
        } else if chord("mod+q") {
            tx.quit();
        } else {
            return false;
        }
        true
    }
}

/// Icon lookups run on workers and land in a burst after a load. Arrivals while a repaint is
/// queued share it, so the palette isn't rebuilt on the UI thread once per icon.
fn palette(shared: &Shared, detail: &DayDetail) -> Palette {
    let dash = shared.ui.get().map(|ui| ui.dash.clone());
    let queued = Arc::new(AtomicBool::new(false));
    Palette::build(detail, &shared.icons, shared.demo, move || {
        if let Some(dash) = &dash
            && !queued.swap(true, Ordering::AcqRel)
        {
            let queued = queued.clone();
            dash.update_if(move |d, _| {
                queued.store(false, Ordering::Release);
                d.repaint()
            });
        }
    })
}

/// Whether a tile with this id can still be drawn from `detail`.
fn cell_exists(detail: &DayDetail, id: &str) -> bool {
    detail.devices.iter().any(|d| {
        let Some(rest) = id
            .strip_prefix(d.device.id.as_str())
            .and_then(|r| r.strip_prefix('/'))
        else {
            return false;
        };
        rest == "other"
            || d.items.iter().any(|i| {
                rest == i.key
                    || rest
                        .strip_prefix(i.key.as_str())
                        .and_then(|r| r.strip_prefix('/'))
                        .and_then(|n| n.parse::<usize>().ok())
                        .is_some_and(|n| n < i.sessions.len())
            })
    })
}
