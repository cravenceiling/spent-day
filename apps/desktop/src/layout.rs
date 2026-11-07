use spentday_core::{DayDetail, DayItem, DeviceDay, ItemKind, Session};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn inset(self, gap: f64) -> Self {
        Self::new(
            self.x + gap / 2.0,
            self.y + gap / 2.0,
            (self.w - gap).max(0.0),
            (self.h - gap).max(0.0),
        )
    }
}

pub fn squarify(values: &[f64], bounds: Rect) -> Vec<Rect> {
    let total: f64 = values.iter().filter(|v| **v > 0.0).sum();
    if total <= 0.0 || bounds.w <= 0.0 || bounds.h <= 0.0 {
        return vec![];
    }
    let areas: Vec<_> = values
        .iter()
        .filter(|v| **v > 0.0)
        .map(|v| v * bounds.w * bounds.h / total)
        .collect();
    let mut rect = bounds;
    let mut output = Vec::new();
    let mut index = 0;
    while index < areas.len() {
        let start = index;
        index += 1;
        let side = rect.w.min(rect.h);
        while index < areas.len()
            && worst(&areas[start..=index], side) <= worst(&areas[start..index], side)
        {
            index += 1;
        }
        let sum: f64 = areas[start..index].iter().sum();
        if rect.w >= rect.h {
            let width = sum / rect.h;
            let mut y = rect.y;
            for area in &areas[start..index] {
                let height = area / width;
                output.push(Rect::new(rect.x, y, width, height));
                y += height;
            }
            rect.x += width;
            rect.w = (rect.w - width).max(0.0);
        } else {
            let height = sum / rect.w;
            let mut x = rect.x;
            for area in &areas[start..index] {
                let width = area / height;
                output.push(Rect::new(x, rect.y, width, height));
                x += width;
            }
            rect.y += height;
            rect.h = (rect.h - height).max(0.0);
        }
    }
    output
}

fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let maximum = row.iter().copied().fold(0.0, f64::max);
    let minimum = row.iter().copied().fold(f64::INFINITY, f64::min);
    (side * side * maximum / (sum * sum)).max(sum * sum / (side * side * minimum))
}

pub fn slice(values: &[f64], rect: Rect) -> Vec<Rect> {
    let total: f64 = values.iter().filter(|v| **v > 0.0).sum();
    if total <= 0.0 {
        return vec![];
    }
    let horizontal = rect.w >= rect.h;
    let mut position = if horizontal { rect.x } else { rect.y };
    values
        .iter()
        .filter(|v| **v > 0.0)
        .map(|v| {
            let length = v / total * if horizontal { rect.w } else { rect.h };
            let result = if horizontal {
                Rect::new(position, rect.y, length, rect.h)
            } else {
                Rect::new(rect.x, position, rect.w, length)
            };
            position += length;
            result
        })
        .collect()
}

#[derive(Clone, PartialEq)]
pub struct MapItem {
    pub id: String,
    pub device_id: String,
    pub item: DayItem,
    pub hidden: Vec<DayItem>,
    pub session: Option<Session>,
}
impl MapItem {
    pub fn total_ms(&self) -> i64 {
        self.session
            .as_ref()
            .map_or(self.item.total_ms, |s| s.end_ms - s.start_ms)
    }
    pub fn sessions(&self) -> &[Session] {
        self.session
            .as_ref()
            .map_or(&self.item.sessions, std::slice::from_ref)
    }
}

pub struct MapGroup {
    pub device: DeviceDay,
    pub label: Rect,
    pub tiles: Vec<(MapItem, Rect)>,
}

pub fn map_items(device: &DeviceDay, day_total: i64) -> Vec<MapItem> {
    let (small, large): (Vec<_>, Vec<_>) = device
        .items
        .iter()
        .cloned()
        .partition(|i| (i.total_ms as f64) < day_total as f64 * 0.003);
    let mut items: Vec<_> = large
        .into_iter()
        .map(|item| MapItem {
            id: format!("{}/{}", device.device.id, item.key),
            device_id: device.device.id.clone(),
            item,
            hidden: vec![],
            session: None,
        })
        .collect();
    if !small.is_empty() {
        let mut sessions: Vec<_> = small.iter().flat_map(|i| i.sessions.clone()).collect();
        sessions.sort_by_key(|s| s.start_ms);
        items.push(MapItem {
            id: format!("{}/other", device.device.id),
            device_id: device.device.id.clone(),
            item: DayItem {
                key: "other".into(),
                name: "Other".into(),
                kind: ItemKind::App,
                app_key: "other".into(),
                app_ids: vec![],
                total_ms: small.iter().map(|i| i.total_ms).sum(),
                sessions,
            },
            hidden: small,
            session: None,
        });
    }
    items.sort_by(|a, b| b.total_ms().cmp(&a.total_ms()).then(a.id.cmp(&b.id)));
    items
}

pub fn device_groups(detail: &DayDetail, bounds: Rect) -> Vec<MapGroup> {
    let mut devices = detail.devices.clone();
    devices.sort_by(|a, b| {
        b.total_ms
            .cmp(&a.total_ms)
            .then(a.device.id.cmp(&b.device.id))
    });
    let boxes = squarify(
        &devices
            .iter()
            .map(|d| d.total_ms as f64)
            .collect::<Vec<_>>(),
        bounds.inset(-18.0),
    );
    devices
        .into_iter()
        .zip(boxes)
        .map(|(device, box_)| {
            let rect = box_.inset(18.0);
            let items = map_items(&device, detail.wall_ms);
            let inner = Rect::new(
                rect.x - 2.0,
                rect.y + 22.0,
                rect.w + 4.0,
                (rect.h - 20.0).max(0.0),
            );
            let boxes = squarify(
                &items
                    .iter()
                    .map(|i| i.total_ms() as f64)
                    .collect::<Vec<_>>(),
                inner,
            );
            let tiles = items
                .into_iter()
                .zip(boxes)
                .map(|(i, r)| (i, r.inset(4.0)))
                .collect();
            MapGroup {
                device,
                label: Rect::new(rect.x, rect.y, rect.w, 24.0),
                tiles,
            }
        })
        .collect()
}

pub fn union_sessions(sessions: impl IntoIterator<Item = Session>) -> Vec<Session> {
    let mut sessions: Vec<_> = sessions.into_iter().collect();
    sessions.sort_by_key(|s| s.start_ms);
    let mut result: Vec<Session> = vec![];
    for session in sessions {
        if let Some(last) = result
            .last_mut()
            .filter(|last| session.start_ms <= last.end_ms)
        {
            last.end_ms = last.end_ms.max(session.end_ms);
        } else {
            result.push(session);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use spentday_core::{DeviceInfo, Platform};
    fn rects(actual: Vec<Rect>, expected: Vec<Rect>) {
        assert_eq!(actual.len(), expected.len());
        for (a, b) in actual.into_iter().zip(expected) {
            for (x, y) in [a.x, a.y, a.w, a.h].into_iter().zip([b.x, b.y, b.w, b.h]) {
                assert!((x - y).abs() < 0.00001, "{a:?} != {b:?}");
            }
        }
    }
    #[test]
    fn literal_layouts() {
        rects(
            squarify(&[6., 6., 4., 3., 2., 2., 1.], Rect::new(0., 0., 6., 4.)),
            vec![
                Rect::new(0., 0., 3., 2.),
                Rect::new(0., 2., 3., 2.),
                Rect::new(3., 0., 12. / 7., 7. / 3.),
                Rect::new(33. / 7., 0., 9. / 7., 7. / 3.),
                Rect::new(3., 7. / 3., 1.2, 5. / 3.),
                Rect::new(4.2, 7. / 3., 1.2, 5. / 3.),
                Rect::new(5.4, 7. / 3., 0.6, 5. / 3.),
            ],
        );
        rects(
            squarify(&[3., 1.], Rect::new(0., 0., 100., 100.)),
            vec![Rect::new(0., 0., 75., 100.), Rect::new(75., 0., 25., 100.)],
        );
        rects(
            squarify(&[0., 1., 0.], Rect::new(5., 7., 30., 40.)),
            vec![Rect::new(5., 7., 30., 40.)],
        );
        assert!(squarify(&[0., 0.], Rect::default()).is_empty());
        rects(
            vec![Rect::new(0., 0., 100., 80.).inset(4.)],
            vec![Rect::new(2., 2., 96., 76.)],
        );
        rects(
            slice(&[1., 3.], Rect::new(0., 0., 100., 50.)),
            vec![Rect::new(0., 0., 25., 50.), Rect::new(25., 0., 75., 50.)],
        );
        rects(
            slice(&[1., 3.], Rect::new(0., 0., 50., 100.)),
            vec![Rect::new(0., 0., 50., 25.), Rect::new(0., 25., 50., 75.)],
        );
    }
    #[test]
    fn bands_gaps_and_other() {
        let item = DayItem {
            key: "app:a".into(),
            name: "A".into(),
            kind: ItemKind::App,
            app_key: "a".into(),
            app_ids: vec![],
            total_ms: 100,
            sessions: vec![Session {
                start_ms: 0,
                end_ms: 100,
            }],
        };
        let device = DeviceDay {
            device: DeviceInfo {
                id: "a".into(),
                name: "Device".into(),
                platform: Platform::Macos,
                last_seen_ms: 0,
            },
            total_ms: 100,
            items: vec![item.clone()],
        };
        let mut detail = DayDetail {
            start_ms: 0,
            end_ms: 100,
            wall_ms: 100,
            device_ms: 100,
            devices: vec![device.clone()],
        };
        let groups = device_groups(&detail, Rect::new(0., 0., 100., 100.));
        rects(
            vec![groups[0].label, groups[0].tiles[0].1],
            vec![Rect::new(0., 0., 100., 24.), Rect::new(0., 24., 100., 76.)],
        );
        let mut second = device.clone();
        second.device.id = "b".into();
        detail.devices.push(second);
        rects(
            device_groups(&detail, Rect::new(0., 0., 218., 100.))
                .iter()
                .map(|g| g.label)
                .collect(),
            vec![Rect::new(0., 0., 100., 24.), Rect::new(118., 0., 100., 24.)],
        );
        let mut tiny = item;
        tiny.key = "app:tiny".into();
        tiny.total_ms = 2;
        let mut device = device;
        device.items.push(tiny);
        let items = map_items(&device, 1000);
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].item.name, "Other");
        assert_eq!(items[1].hidden.len(), 1);
    }
    #[test]
    fn union_counts_overlap_once() {
        let union = union_sessions([
            Session {
                start_ms: 20,
                end_ms: 40,
            },
            Session {
                start_ms: 0,
                end_ms: 30,
            },
            Session {
                start_ms: 60,
                end_ms: 80,
            },
        ]);
        assert_eq!(
            union,
            vec![
                Session {
                    start_ms: 0,
                    end_ms: 40
                },
                Session {
                    start_ms: 60,
                    end_ms: 80
                }
            ]
        );
    }
}
