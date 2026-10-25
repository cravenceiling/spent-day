//! Time per hour (or per day) as a bar chart, drawn with cairo.

use crate::format;
use crate::range::{HOUR_MS, Range};
use adw::prelude::*;
use gtk::{cairo, glib};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

const TOP: f64 = 10.0;
const AXIS: f64 = 24.0;
const GUTTER: f64 = 44.0;

#[derive(Default)]
struct Bars {
    values: Vec<i64>,
    /// Axis label under each bar, empty for bars without one.
    labels: Vec<String>,
    tips: Vec<String>,
}

pub struct Chart {
    pub area: gtk::DrawingArea,
    bars: Rc<RefCell<Bars>>,
}

impl Chart {
    pub fn new() -> Chart {
        let area = gtk::DrawingArea::builder().content_height(190).hexpand(true).has_tooltip(true).build();
        let bars = Rc::new(RefCell::new(Bars::default()));
        area.set_draw_func({
            let bars = bars.clone();
            move |area, cr, w, h| draw(area, cr, w as f64, h as f64, &bars.borrow())
        });
        area.connect_query_tooltip({
            let bars = bars.clone();
            move |area, x, _y, _keyboard, tooltip| {
                let bars = bars.borrow();
                let slot = (area.width() as f64 - GUTTER) / bars.values.len().max(1) as f64;
                let tip = bars.tips.get((x as f64 / slot) as usize);
                tooltip.set_text(tip.map(String::as_str));
                tip.is_some()
            }
        });
        adw::StyleManager::default().connect_accent_color_notify({
            let area = area.clone();
            move |_| area.queue_draw()
        });
        Chart { area, bars }
    }

    pub fn set(&self, range: Range, start: &glib::DateTime, values: Vec<i64>) {
        let (labels, tips) = values
            .iter()
            .enumerate()
            .map(|(i, &ms)| {
                if range == Range::Week {
                    let day = start.add_days(i as i32).expect("day in range");
                    let name = day.format("%a").map(|s| s.to_string()).unwrap_or_default();
                    let tip = day.format("%A").map(|s| s.to_string()).unwrap_or_default();
                    (name, format!("{tip} · {}", format::duration(ms)))
                } else {
                    let label = if i % 6 == 0 { format!("{i:02}:00") } else { String::new() };
                    (label, format!("{i:02}:00 · {}", format::duration(ms)))
                }
            })
            .unzip();
        *self.bars.borrow_mut() = Bars { values, labels, tips };
        self.area.queue_draw();
    }
}

/// The smallest round duration at or above `ms` whose half is also round, since the chart
/// labels the top and middle gridlines.
fn ceiling(ms: i64) -> i64 {
    const MINUTES: [i64; 14] = [2, 4, 10, 20, 30, 60, 90, 120, 180, 240, 360, 480, 720, 960];
    MINUTES
        .iter()
        .map(|m| m * 60_000)
        .find(|&step| step >= ms)
        .unwrap_or_else(|| (ms + 4 * HOUR_MS - 1) / (4 * HOUR_MS) * 4 * HOUR_MS)
}

fn draw(area: &gtk::DrawingArea, cr: &cairo::Context, w: f64, h: f64, bars: &Bars) {
    if bars.values.is_empty() {
        return;
    }
    let fg = area.color();
    let accent = adw::StyleManager::default().accent_color_rgba();
    let ink = |alpha: f32| cr.set_source_rgba(fg.red().into(), fg.green().into(), fg.blue().into(), (fg.alpha() * alpha).into());
    let plot_w = w - GUTTER;
    let plot_h = h - TOP - AXIS;
    let max = ceiling(bars.values.iter().copied().max().unwrap_or(0).max(1));

    cr.set_line_width(1.0);
    for (i, fraction) in [1.0, 0.5, 0.0].into_iter().enumerate() {
        let y = (TOP + plot_h * (1.0 - fraction)).round() + 0.5;
        ink(if fraction == 0.0 { 0.18 } else { 0.08 });
        cr.set_dash(if fraction == 0.0 { &[] } else { &[3.0, 3.0] }, 0.0);
        cr.move_to(0.0, y);
        cr.line_to(plot_w, y);
        let _ = cr.stroke();
        if i < 2 {
            ink(0.55);
            text(area, cr, &format::duration((max as f64 * fraction) as i64), plot_w + 8.0, y, Align::Middle);
        }
    }
    cr.set_dash(&[], 0.0);

    let slot = plot_w / bars.values.len() as f64;
    let bar_w = (slot * 0.62).clamp(2.0, 36.0);
    for (i, &ms) in bars.values.iter().enumerate() {
        let x = i as f64 * slot + (slot - bar_w) / 2.0;
        if ms <= 0 {
            ink(0.08);
            rounded_top(cr, x, TOP + plot_h - 2.0, bar_w, 2.0);
        } else {
            let bar_h = (plot_h * ms as f64 / max as f64).max(3.0);
            cr.set_source_rgba(accent.red().into(), accent.green().into(), accent.blue().into(), 1.0);
            rounded_top(cr, x, TOP + plot_h - bar_h, bar_w, bar_h);
        }
        let _ = cr.fill();
        if let Some(label) = bars.labels.get(i).filter(|l| !l.is_empty()) {
            ink(0.55);
            text(area, cr, label, x + bar_w / 2.0, TOP + plot_h + 6.0, Align::Center);
        }
    }
}

fn rounded_top(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    let r = 4.0f64.min(w / 2.0).min(h);
    cr.new_sub_path();
    cr.move_to(x, y + h);
    cr.line_to(x, y + r);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.arc(x + w - r, y + r, r, 1.5 * PI, 0.0);
    cr.line_to(x + w, y + h);
    cr.close_path();
}

enum Align {
    /// Left edge at x, vertically centred on y.
    Middle,
    /// Centred on x, top edge at y.
    Center,
}

fn text(area: &gtk::DrawingArea, cr: &cairo::Context, s: &str, x: f64, y: f64, align: Align) {
    let layout = area.create_pango_layout(None);
    layout.set_markup(&format!("<span size=\"small\" font_features=\"tnum\">{}</span>", glib::markup_escape_text(s)));
    let (tw, th) = layout.pixel_size();
    let (lx, ly) = match align {
        Align::Middle => (x, y - th as f64 / 2.0),
        Align::Center => ((x - tw as f64 / 2.0).max(0.0), y),
    };
    cr.move_to(lx, ly);
    pangocairo::functions::show_layout(cr, &layout);
}

#[cfg(test)]
mod tests {
    use super::ceiling;

    #[test]
    fn rounds_up_to_readable_steps() {
        assert_eq!(ceiling(1), 2 * 60_000);
        assert_eq!(ceiling(3 * 60_000), 4 * 60_000);
        assert_eq!(ceiling(42 * 60_000), 60 * 60_000);
        assert_eq!(ceiling(60 * 60_000), 60 * 60_000);
        assert_eq!(ceiling(7 * 3_600_000 + 1), 8 * 3_600_000);
        assert_eq!(ceiling(17 * 3_600_000), 20 * 3_600_000);
    }
}
