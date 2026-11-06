//! The treemap as data: device headers and tiles with rects, colours, labels and stable ids.
//! Pure, so tests run it without a window. Stub until unit 1.

use crate::colors::Rgb;
use crate::layout::Rect;
use crate::palette::Palette;
use crate::ui::dash::{CellId, Drill};
use spentday_core::{DayDetail, Session};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapLayout {
    /// Emitted before the cells, in focus order: per device its header, then its tiles.
    pub headers: Vec<Header>,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub device: String,
    pub name: String,
    pub total_ms: i64,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub id: CellId,
    pub kind: CellKind,
    /// Already inset, rounded per edge.
    pub rect: Rect,
    pub name: String,
    pub total_ms: i64,
    /// Start-sorted.
    pub sessions: Vec<Session>,
    pub fill: Rgb,
    pub dimmed: Rgb,
    pub ink: Rgb,
    pub label: Label,
    /// `min(10, w/3, h/3)`.
    pub radius: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellKind {
    Item {
        device: String,
        key: String,
    },
    Other {
        device: String,
        hidden: Vec<(String, i64)>,
    },
    Session {
        device: String,
        key: String,
        index: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Label {
    None,
    NameOnly,
    Both { pad: f32, name_pt: f32, dur_pt: f32 },
}

/// Text size for `(text, point size, weight)`; the view passes `rune::text::measure`, tests a
/// fixed-width fake.
pub type Measure<'a> = &'a dyn Fn(&str, f32, f32) -> (f32, f32);

pub fn layout(
    detail: &DayDetail,
    drill: &Drill,
    palette: &Palette,
    size: [f32; 2],
    dark: bool,
    measure: Measure,
) -> MapLayout {
    let _ = (detail, drill, palette, size, dark, measure);
    MapLayout::default()
}
