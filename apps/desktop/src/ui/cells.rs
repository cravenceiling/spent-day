//! The treemap as data: device headers and tiles with rects, colours, labels and stable ids.
//! Pure, so tests run it without a window.

use crate::colors::Rgb;
use crate::layout::{self, MapItem, Rect};
use crate::palette::Palette;
use crate::ui::dash::{CellId, Drill};
use crate::{day, format};
use jiff::tz::TimeZone;
use spentday_core::{DayDetail, Session};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapLayout {
    /// Device rank order; `focus_order` interleaves each header with its tiles.
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
    pub dim_ink: Rgb,
    pub gradient: [Rgb; 2],
    pub dim_gradient: [Rgb; 2],
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

/// Session tiles read their clock span in `tz`.
pub fn layout_in_timezone(
    detail: &DayDetail,
    drill: &Drill,
    palette: &Palette,
    size: [f32; 2],
    dark: bool,
    measure: Measure,
    tz: &TimeZone,
) -> MapLayout {
    let mut output = MapLayout::default();
    if size[0] <= 0. || size[1] <= 0. {
        return output;
    }
    let bounds = Rect::new(0., 0., f64::from(size[0]), f64::from(size[1]));
    let device_id = match drill {
        Drill::Day => None,
        Drill::Device(device) | Drill::Item { device, .. } => Some(device),
    };
    let device = detail
        .devices
        .iter()
        .find(|d| Some(&d.device.id) == device_id);
    let tiles: Vec<_> = if let Some(device) = device {
        let bounds = bounds.inset(-4.);
        if let Drill::Item { key, .. } = drill
            && let Some(item) = device.items.iter().find(|i| i.key == *key)
        {
            let mut sessions = item.sessions.clone();
            sessions.sort_by_key(|s| s.start_ms);
            let rects = layout::slice(
                &sessions
                    .iter()
                    .map(|s| (s.end_ms - s.start_ms) as f64)
                    .collect::<Vec<_>>(),
                bounds,
            );
            sessions
                .into_iter()
                .zip(rects)
                .enumerate()
                .map(|(index, (session, rect))| {
                    (
                        MapItem {
                            id: format!("{}/{}/{index}", device.device.id, item.key),
                            device_id: device.device.id.clone(),
                            item: item.clone(),
                            hidden: Vec::new(),
                            session: Some(session),
                        },
                        rect.inset(4.),
                        Some(index),
                    )
                })
                .collect()
        } else {
            let items = layout::map_items(device, detail.wall_ms);
            let rects = layout::squarify(
                &items
                    .iter()
                    .map(|i| i.total_ms() as f64)
                    .collect::<Vec<_>>(),
                bounds,
            );
            items
                .into_iter()
                .zip(rects)
                .map(|(item, rect)| (item, rect.inset(4.), None))
                .collect()
        }
    } else {
        layout::device_groups(detail, bounds)
            .into_iter()
            .flat_map(|group| {
                output.headers.push(Header {
                    device: group.device.device.id,
                    name: group.device.device.name,
                    total_ms: group.device.total_ms,
                    rect: group.label,
                });
                group
                    .tiles
                    .into_iter()
                    .map(|(item, rect)| (item, rect, None))
            })
            .collect()
    };
    for (item, rect, index) in tiles {
        let x = rect.x.round();
        let y = rect.y.round();
        let rect = Rect::new(
            x,
            y,
            ((rect.x + rect.w).round() - x).max(0.),
            ((rect.y + rect.h).round() - y).max(0.),
        );
        if rect.w <= 0. || rect.h <= 0. {
            continue;
        }
        let total_ms = item.total_ms();
        let name = item.session.as_ref().map_or_else(
            || item.item.name.clone(),
            |s| {
                format!(
                    "{} to {}",
                    day::clock(s.start_ms, detail.end_ms, tz),
                    day::clock(s.end_ms, detail.end_ms, tz)
                )
            },
        );
        let duration = format::duration(total_ms);
        let mut label = Label::None;
        let tiers = [(16., 22., 15.), (13., 15., 13.), (12., 13., 12.5)];
        let first_tier = usize::from(rect.w < 260. || rect.h < 200.);
        for &(pad, name_pt, dur_pt) in &tiers[first_tier..] {
            // The weights the tile draws with.
            let (nw, _) = measure(&name, name_pt, 650.);
            let (dw, _) = measure(&duration, dur_pt, 500.);
            if rect.w as f32 - 2. * pad >= nw.max(dw) + 2.
                && rect.h as f32 - 2. * pad >= (name_pt + dur_pt) * 1.4 + 2.
            {
                label = Label::Both {
                    pad,
                    name_pt,
                    dur_pt,
                };
                break;
            }
        }
        if label == Label::None {
            let (width, height) = measure(&name, 13., 650.);
            if rect.w as f32 - 24. >= width + 2. && rect.h as f32 - 20. >= height {
                label = Label::NameOnly;
            }
        }
        let kind = if let Some(index) = index {
            CellKind::Session {
                device: item.device_id.clone(),
                key: item.item.key.clone(),
                index,
            }
        } else if !item.hidden.is_empty() {
            CellKind::Other {
                device: item.device_id.clone(),
                hidden: item
                    .hidden
                    .iter()
                    .map(|i| (i.name.clone(), i.total_ms))
                    .collect(),
            }
        } else {
            CellKind::Item {
                device: item.device_id.clone(),
                key: item.item.key.clone(),
            }
        };
        let style = index.map_or_else(
            || palette.style(&item.item.key, dark),
            |i| palette.session_style(&item.item.key, i, dark),
        );
        let mut sessions = item.sessions().to_vec();
        sessions.sort_by_key(|s| s.start_ms);
        output.cells.push(Cell {
            id: item.id,
            kind,
            rect,
            name,
            total_ms,
            sessions,
            fill: style.base,
            dimmed: style.dim_fill,
            ink: style.ink,
            dim_ink: style.dim_ink,
            gradient: style.gradient,
            dim_gradient: style.dim_gradient,
            label,
            radius: 10_f32.min(rect.w as f32 / 3.).min(rect.h as f32 / 3.),
        });
    }
    output
}

impl MapLayout {
    /// View keys in roving order: each device header followed by that device's ranked tiles.
    pub fn focus_order(&self) -> Vec<String> {
        if self.headers.is_empty() {
            return self
                .cells
                .iter()
                .map(|c| format!("tile-{}", c.id))
                .collect();
        }
        self.headers
            .iter()
            .flat_map(|header| {
                std::iter::once(format!("hdr-{}", header.device)).chain(
                    self.cells
                        .iter()
                        .filter(|cell| match &cell.kind {
                            CellKind::Item { device, .. }
                            | CellKind::Other { device, .. }
                            | CellKind::Session { device, .. } => *device == header.device,
                        })
                        .map(|c| format!("tile-{}", c.id)),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spentday_core::{DayItem, DeviceDay, DeviceInfo, ItemKind, Platform};

    fn layout(
        detail: &DayDetail,
        drill: &Drill,
        palette: &Palette,
        size: [f32; 2],
        dark: bool,
        measure: Measure,
    ) -> MapLayout {
        layout_in_timezone(detail, drill, palette, size, dark, measure, &TimeZone::UTC)
    }

    fn measure(text: &str, pt: f32, _: f32) -> (f32, f32) {
        (text.chars().count() as f32 * pt * 0.5, pt)
    }

    fn fixture() -> DayDetail {
        DayDetail {
            start_ms: 0,
            end_ms: 1000,
            wall_ms: 1000,
            device_ms: 1000,
            devices: vec![DeviceDay {
                device: DeviceInfo {
                    id: "a".into(),
                    name: "Device".into(),
                    platform: Platform::Macos,
                    last_seen_ms: 0,
                },
                total_ms: 1000,
                items: vec![DayItem {
                    key: "app:a".into(),
                    name: "A".into(),
                    kind: ItemKind::App,
                    app_key: "a".into(),
                    app_ids: vec![],
                    total_ms: 1000,
                    sessions: vec![Session {
                        start_ms: 0,
                        end_ms: 1000,
                    }],
                }],
            }],
        }
    }

    #[test]
    fn ids_and_other_threshold_use_whole_day_wall_in_device_drill() {
        let mut detail = fixture();
        let mut tiny = detail.devices[0].items[0].clone();
        tiny.key = "app:tiny".into();
        tiny.name = "Tiny".into();
        tiny.total_ms = 2;
        tiny.sessions = vec![Session {
            start_ms: 30,
            end_ms: 32,
        }];
        let mut threshold = tiny.clone();
        threshold.key = "app:threshold".into();
        threshold.total_ms = 3;
        detail.devices[0].items.extend([tiny, threshold]);
        detail.devices[0].total_ms = 1005;
        for drill in [Drill::Day, Drill::Device("a".into())] {
            let map = layout(
                &detail,
                &drill,
                &Palette::default(),
                [1000., 1000.],
                true,
                &measure,
            );
            assert_eq!(
                map.cells.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
                ["a/app:a", "a/app:threshold", "a/other"]
            );
            let other = &map.cells[2];
            assert_eq!(other.total_ms, 2);
            assert_eq!(
                other.sessions,
                [Session {
                    start_ms: 30,
                    end_ms: 32
                }]
            );
            assert_eq!(
                other.kind,
                CellKind::Other {
                    device: "a".into(),
                    hidden: vec![("Tiny".into(), 2)]
                }
            );
        }
    }

    #[test]
    fn session_slices_are_chronological_with_sorted_indices() {
        let mut detail = fixture();
        detail.devices[0].items[0].sessions = vec![
            Session {
                start_ms: 20,
                end_ms: 50,
            },
            Session {
                start_ms: 0,
                end_ms: 10,
            },
        ];
        let drill = Drill::Item {
            device: "a".into(),
            key: "app:a".into(),
        };
        for size in [[100., 50.], [50., 100.]] {
            let map = layout_in_timezone(
                &detail,
                &drill,
                &Palette::default(),
                size,
                true,
                &measure,
                &TimeZone::UTC,
            );
            assert!(map.headers.is_empty());
            assert_eq!(
                map.cells.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
                ["a/app:a/0", "a/app:a/1"]
            );
            assert_eq!(
                map.cells.iter().map(|c| c.total_ms).collect::<Vec<_>>(),
                [10, 30]
            );
            assert_eq!(
                map.cells[0].sessions,
                [Session {
                    start_ms: 0,
                    end_ms: 10
                }]
            );
            assert_eq!(
                map.cells[1].kind,
                CellKind::Session {
                    device: "a".into(),
                    key: "app:a".into(),
                    index: 1
                }
            );
            assert_eq!(
                map.cells[0].rect,
                if size[0] > size[1] {
                    Rect::new(0., 0., 22., 50.)
                } else {
                    Rect::new(0., 0., 50., 22.)
                }
            );
            assert_eq!(map.cells[0].name, "00:00 to 00:00");
            assert_eq!(map.focus_order(), ["tile-a/app:a/0", "tile-a/app:a/1"]);
        }
    }

    #[test]
    fn labels_choose_first_fitting_tier_without_truncation() {
        let mut detail = fixture();
        let palette = Palette::default();
        let drill = Drill::Device("a".into());
        for (size, expected) in [
            (
                [260., 200.],
                Label::Both {
                    pad: 16.,
                    name_pt: 22.,
                    dur_pt: 15.,
                },
            ),
            (
                [120., 96.],
                Label::Both {
                    pad: 13.,
                    name_pt: 15.,
                    dur_pt: 13.,
                },
            ),
            (
                [120., 66.],
                Label::Both {
                    pad: 12.,
                    name_pt: 13.,
                    dur_pt: 12.5,
                },
            ),
            ([120., 40.], Label::NameOnly),
        ] {
            let map = layout(&detail, &drill, &palette, size, true, &measure);
            assert_eq!(map.cells[0].label, expected);
        }
        detail.devices[0].items[0].name = "An unbreakably long application name".into();
        assert_eq!(
            layout(&detail, &drill, &palette, [72., 96.], true, &measure).cells[0].label,
            Label::None
        );
        assert!(
            layout(&detail, &drill, &palette, [0., 96.], true, &measure)
                .cells
                .is_empty()
        );
    }

    #[test]
    fn headers_precede_their_own_ranked_tiles_in_focus_order() {
        let tz = TimeZone::UTC;
        let mut detail = crate::demo::Demo::with_timezone(tz.clone())
            .day(day::query(jiff::civil::date(2026, 10, 28), &tz));
        detail.devices.reverse();
        for d in &mut detail.devices {
            d.items.reverse();
        }
        let map = layout(
            &detail,
            &Drill::Day,
            &Palette::default(),
            [1208., 450.],
            true,
            &measure,
        );
        assert_eq!(
            map.headers
                .iter()
                .map(|h| h.device.as_str())
                .collect::<Vec<_>>(),
            ["demo-mac", "demo-phone", "demo-linux"]
        );
        let order = map.focus_order();
        assert_eq!(
            &order[..3],
            [
                "hdr-demo-mac",
                "tile-demo-mac/app:com.apple.dt.Xcode",
                "tile-demo-mac/site:github.com"
            ]
        );
        assert_eq!(
            &order[7..10],
            [
                "hdr-demo-phone",
                "tile-demo-phone/app:instagram",
                "tile-demo-phone/app:youtube"
            ]
        );
        assert_eq!(
            &order[12..],
            [
                "hdr-demo-linux",
                "tile-demo-linux/app:vscode",
                "tile-demo-linux/site:news.ycombinator.com"
            ]
        );
        assert_eq!(order.len(), map.headers.len() + map.cells.len());
        for cell in map.cells {
            assert_eq!(cell.rect.x.fract(), 0.);
            assert_eq!(cell.rect.y.fract(), 0.);
            assert_eq!(cell.rect.w.fract(), 0.);
            assert_eq!(cell.rect.h.fract(), 0.);
            assert!(
                cell.radius <= 10.
                    && cell.radius <= cell.rect.w as f32 / 3.
                    && cell.radius <= cell.rect.h as f32 / 3.
            );
            assert!(
                cell.fill.contrast(cell.ink) >= 4.5 && cell.dimmed.contrast(cell.dim_ink) >= 4.5
            );
        }
    }

    #[test]
    fn missing_drills_fall_back_without_losing_geometry() {
        let detail = fixture();
        let palette = Palette::default();
        let map = |drill: &Drill| layout(&detail, drill, &palette, [100., 100.], true, &measure);
        assert_eq!(map(&Drill::Device("missing".into())), map(&Drill::Day));
        assert_eq!(
            map(&Drill::Item {
                device: "a".into(),
                key: "missing".into()
            }),
            map(&Drill::Device("a".into()))
        );
        let day = map(&Drill::Day);
        assert_eq!(day.headers[0].rect, Rect::new(0., 0., 100., 24.));
        assert_eq!(day.cells[0].rect, Rect::new(0., 24., 100., 76.));
    }
}
