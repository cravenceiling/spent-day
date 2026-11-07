//! The day strip: every session on one midnight-to-midnight track, and the popover's mini strip
//! for one tile. Ticks sit at local 00/06/12/18/24, so a DST day's ticks fall where the clock
//! says, not at even quarters.

use crate::ui::cells::Cell;
use crate::ui::dash::{Drill, Loaded};
use crate::ui::dash_view::Page;
use crate::ui::treemap::{color, device_of};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use rune::{El, TextStyle, el, text};
use spentday_core::Session;

const TICK: TextStyle = TextStyle::new(11.5, 500.0);

/// The full strip. Sessions outside the drilled device or item fade, and with `hot` only that
/// tile's sessions stay bright.
pub fn strip(
    loaded: &Loaded,
    drill: &Drill,
    hot: Option<&Cell>,
    width: f32,
    tz: &TimeZone,
    dark: bool,
) -> El {
    let detail = &loaded.detail;
    let mut sessions: Vec<(&str, &str, &Session)> = detail
        .devices
        .iter()
        .flat_map(|d| {
            d.items.iter().flat_map(move |i| {
                i.sessions
                    .iter()
                    .map(move |s| (d.device.id.as_str(), i.key.as_str(), s))
            })
        })
        .collect();
    sessions.sort_by_key(|(_, _, s)| s.start_ms);
    let bars = sessions.into_iter().map(|(device, key, s)| {
        let in_scope = match drill {
            Drill::Day => true,
            Drill::Device(d) => d == device,
            Drill::Item { device: d, key: k } => d == device && k == key,
        };
        let bright =
            in_scope && hot.is_none_or(|c| device_of(c) == device && c.sessions.contains(s));
        (key, s, bright)
    });
    el().key("strip")
        .role("img")
        .label("Screen time across the day, midnight to midnight")
        .w_full()
        .h(30.0)
        .child(track(loaded, bars, width, 10.0, dark))
        .children(ticks(loaded, width, 16.0, tz, dark))
}

/// The popover's strip for one tile.
pub fn mini(loaded: &Loaded, cell: &Cell, width: f32, tz: &TimeZone, dark: bool) -> El {
    let key = crate::ui::treemap::palette_key(cell);
    let bars = cell.sessions.iter().map(|s| (key, s, true));
    el().key("mini-strip")
        .size(width, 26.0)
        .child(track(loaded, bars, width, 8.0, dark))
        .children(ticks(loaded, width, 12.0, tz, dark))
}

fn track<'a>(
    loaded: &Loaded,
    bars: impl Iterator<Item = (&'a str, &'a Session, bool)>,
    width: f32,
    h: f32,
    dark: bool,
) -> El {
    let d = &loaded.detail;
    let span = (d.end_ms - d.start_ms).max(1) as f32;
    let page = Page::of(dark);
    el().absolute(0.0, 0.0)
        .size(width, h)
        .radius(h / 2.0)
        .fill(page.track)
        .clip_rounded(h / 2.0)
        .children(bars.map(|(key, s, bright)| {
            let fill = loaded.palette.fill(key, dark);
            let x = (s.start_ms - d.start_ms) as f32 / span * width;
            let w = ((s.end_ms - s.start_ms) as f32 / span * width).max(2.0);
            el().absolute(x, 0.0)
                .size(w, h)
                .radius(2.0)
                .fill(color(fill))
                .opacity(if bright { 1.0 } else { 0.2 })
        }))
}

fn ticks(loaded: &Loaded, width: f32, y: f32, tz: &TimeZone, dark: bool) -> Vec<El> {
    let d = &loaded.detail;
    let span = (d.end_ms - d.start_ms).max(1) as f32;
    let page = Page::of(dark);
    let date = Timestamp::from_millisecond(d.start_ms)
        .map(|t| t.to_zoned(tz.clone()).date())
        .unwrap_or_default();
    [0i8, 6, 12, 18, 24]
        .into_iter()
        .map(|hour| {
            let label = format!("{hour:02}");
            let (tw, _) = rune::text::measure(&label, &[], TICK, None);
            let x = match hour {
                0 => 0.0,
                24 => width - tw,
                _ => {
                    let at = date
                        .at(hour, 0, 0, 0)
                        .to_zoned(tz.clone())
                        .map_or(d.start_ms, |z| z.timestamp().as_millisecond());
                    (at - d.start_ms) as f32 / span * width - tw / 2.0
                }
            };
            text(label).font(TICK).color(page.faint).absolute(x, y)
        })
        .collect()
}
