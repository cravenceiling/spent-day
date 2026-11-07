//! The menu bar's compact summary. Linux registers it for scene renders too.

use crate::app::Shared;
use crate::palette::{Palette, TileStyle};
use crate::platform;
use crate::sync::{self, SyncView, Trigger};
use crate::ui::{bg, font, kit, settings};
use rune::{Color, Cx, El, ImageCache, Key, KeyEvent, Tx, col, el, row, text};
use spentday_core::SummaryQuery;
use std::sync::Arc;

pub const SIZE: (f32, f32) = (264.0, 350.0);

pub struct Panel {
    pub shared: Arc<Shared>,
    pub total_ms: i64,
    pub rows: Vec<PanelRow>,
    pub palette: Palette,
    pub sync: SyncView,
    pub generation: u64,
    images: Option<Arc<ImageCache>>,
}

pub struct PanelRow {
    pub key: String,
    pub name: String,
    pub total_ms: i64,
    pub icon: Option<rune::Image>,
}

impl Panel {
    pub fn new(shared: Arc<Shared>) -> Panel {
        Panel {
            shared,
            total_ms: 0,
            rows: Vec::new(),
            palette: Palette::default(),
            sync: SyncView::default(),
            generation: 0,
            images: None,
        }
    }

    pub fn refresh(&mut self, _tx: &mut Tx) -> bool {
        let Some(ui) = self.shared.ui.get() else {
            return false;
        };
        self.generation += 1;
        let generation = self.generation;
        let to = ui.panel.clone();
        let images = self
            .images
            .get_or_insert_with(|| {
                let to = to.clone();
                Arc::new(ImageCache::new(Arc::new(move || to.wake())))
            })
            .clone();
        let shared = self.shared.clone();
        crate::jobs::spawn(
            to,
            move || {
                let q = crate::day::query(crate::day::today(&shared.tz), &shared.tz);
                let detail = shared
                    .store
                    .day(q.clone())
                    .map_err(|e| crate::format::error("load today's total", &e))?;
                let palette = Palette::build(&detail, &shared.icons, shared.demo, || {});
                let summary = shared
                    .store
                    .summary(SummaryQuery {
                        start_ms: q.start_ms,
                        end_ms: q.end_ms,
                        bucket_ms: 3_600_000,
                        device_id: None,
                    })
                    .map_err(|e| crate::format::error("load today's apps", &e))?;
                let rows = summary
                    .apps
                    .into_iter()
                    .take(3)
                    .map(|app| {
                        let icon = platform::icon_path(&app.app_key, &app.app_ids)
                            .map(|path| images.app_icon(path, 18.0));
                        PanelRow {
                            key: format!("app:{}", app.app_key),
                            name: app.app_name,
                            total_ms: app.total_ms,
                            icon,
                        }
                    })
                    .collect();
                Ok::<_, String>((detail.wall_ms, rows, palette))
            },
            move |p, result, _| {
                if generation != p.generation {
                    return false;
                }
                if let Ok((total_ms, rows, palette)) = result {
                    p.total_ms = total_ms;
                    p.rows = rows;
                    p.palette = palette;
                    return true;
                }
                false
            },
        );
        false
    }

    pub fn on_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if ev.key == Key::Escape {
            tx.close_window();
            true
        } else if ev.is_command('q') {
            tx.quit();
            true
        } else {
            false
        }
    }
}

fn monogram(name: &str, style: TileStyle) -> El {
    let [top, bottom] = style.gradient.map(|rgb| {
        let lch = rgb.lch();
        rune::color::oklch(lch.l as f32, lch.c as f32, lch.h as f32)
    });
    let ink = if style.ink == crate::colors::Rgb::WHITE {
        Color::WHITE
    } else {
        Color::BLACK
    };
    el().size(18.0, 18.0).center().child(
        el().size(15.12, 15.12)
            .radius(4.32)
            .fill(bottom)
            .linear_gradient(std::f32::consts::PI, [(0.0, top), (1.0, bottom)])
            .center()
            .child(
                text(
                    name.chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_else(|| "?".into()),
                )
                .font(rune::TextStyle::new(8.3, 650.0))
                .color(ink),
            ),
    )
}

pub fn view(s: &Panel, cx: &mut Cx<Panel>) -> El {
    let dark = cx.system_dark();
    let k = kit(dark);
    let top = s.rows.first().map_or(1, |r| r.total_ms.max(1));
    let apps = s.rows.iter().take(3).enumerate().map(|(i, app)| {
        let style = s.palette.style(&app.key, dark);
        let [top_color, bottom_color] = style.gradient.map(|rgb| {
            let lch = rgb.lch();
            rune::color::oklch(lch.l as f32, lch.c as f32, lch.h as f32)
        });
        let icon = match &app.icon {
            Some(icon) if icon.is_ready() && icon.error().is_none() => {
                el().size(18.0, 18.0).pixels(icon.pixels())
            }
            _ => monogram(&app.name, style),
        };
        row()
            .key(format!("panel-app-{i}"))
            .w_full()
            .gap(8.0)
            .center_y()
            .child(icon)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(5.0)
                    .child(
                        text(&app.name)
                            .font(font::BODY)
                            .color(k.c.text_base)
                            .truncate()
                            .w_full(),
                    )
                    .child(
                        el().key(format!("panel-bar-track-{i}"))
                            .w_full()
                            .h(4.0)
                            .radius(2.0)
                            .fill(k.c.layer_03)
                            .child(
                                el().w_frac((app.total_ms.max(0) as f32 / top as f32).min(1.0))
                                    .min_w(3.0)
                                    .h(4.0)
                                    .radius(2.0)
                                    .fill(bottom_color)
                                    .linear_gradient(
                                        std::f32::consts::FRAC_PI_2,
                                        [(0.0, top_color), (1.0, bottom_color)],
                                    ),
                            ),
                    ),
            )
            .child(
                row()
                    .key(format!("panel-duration-{i}"))
                    .w(48.0)
                    .justify_end()
                    .child(
                        text(crate::format::duration(app.total_ms))
                            .font(font::LABEL)
                            .color(k.c.text_muted),
                    ),
            )
    });
    let menu = |label: &str| {
        row()
            .w_full()
            .h(30.0)
            .px(8.0)
            .center_y()
            .radius(5.0)
            .role("button")
            .label(label)
            .focusable(true)
            .hover(k.c.hover)
            .press(k.c.pressed)
            .child(text(label).font(font::BODY).color(k.c.text_base))
    };
    let configured = s.sync.configured;
    let mut sync_row = menu(if s.sync.running {
        "Syncing…"
    } else {
        "Sync Now"
    })
    .key("panel-sync")
    .disabled(!configured || s.sync.running)
    .on_press(|p: &mut Panel| sync::run(&p.shared, Trigger::Manual));
    if !configured {
        sync_row = sync_row.help("Set up sync in the dashboard's settings");
    }
    if s.sync.running {
        sync_row = sync_row
            .child(el().grow(1.0))
            .child(rune_kit::Loader::new().size(12.0).build(k));
    }
    col()
        .key("panel")
        .w_full()
        .h_full()
        .fill(bg(dark))
        .radius(12.0)
        .child(
            col()
                .w_full()
                .px(14.0)
                .pt(14.0)
                .pb(12.0)
                .gap(3.0)
                .child(text("Today").font(font::LABEL).color(k.c.text_muted))
                .child(
                    text(crate::format::duration(s.total_ms))
                        .key("total")
                        .font(rune::TextStyle::new(30.0, 650.0))
                        .color(k.c.text_base),
                ),
        )
        .child(col().w_full().px(14.0).pb(12.0).gap(10.0).children(apps))
        .child(el().w_full().h(1.0).fill(k.c.border_weak))
        .child(
            col()
                .w_full()
                .padding(6.0)
                .child(
                    menu("Open Dashboard")
                        .key("panel-open")
                        .on_press_tx(|p: &mut Panel, tx| {
                            tx.close_window();
                            if let Some(ui) = p.shared.ui.get() {
                                ui.dash.update_if(|d, tx| d.refresh(tx));
                                ui.dash.open();
                            }
                        }),
                )
                .child(sync_row)
                .child(
                    menu("Settings…")
                        .key("panel-settings")
                        .on_press_tx(|p: &mut Panel, tx| {
                            tx.close_window();
                            if let Some(ui) = p.shared.ui.get() {
                                ui.dash.update(settings::open);
                                ui.dash.open();
                            }
                        }),
                )
                .child(el().w_full().h(1.0).my(4.0).fill(k.c.border_weak))
                .child(
                    menu("Quit Spent Day")
                        .key("panel-quit")
                        .on_press_tx(|_: &mut Panel, tx| tx.quit()),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo::Demo, icons};
    use rune::{Env, Runtime};
    use std::sync::{Mutex, OnceLock};

    #[test]
    fn app_rows_align_tracks_and_duration_text_in_both_themes() {
        for dark in [true, false] {
            let shared = Arc::new(Shared {
                store: Arc::new(Demo::new()),
                demo: true,
                tz: jiff::tz::TimeZone::UTC,
                icons: icons::Seeds::default(),
                sync: Mutex::default(),
                ui: OnceLock::new(),
            });
            let mut panel = Panel::new(shared);
            (crate::scenes::PANEL[0].1)(&mut panel);
            let mut rt = Runtime::new(panel, view, SIZE, Env::new(dark, 2.0));
            rt.frame(0.0);
            let elements = rt.elements();
            let mut tracks = Vec::new();
            let mut edges = Vec::new();
            for i in 0..3 {
                let track_key = format!("panel-bar-track-{i}");
                let track = elements
                    .iter()
                    .find(|e| e.key.as_ref() == Some(&track_key))
                    .unwrap();
                tracks.push((track.rect[0], track.rect[2]));
                let duration_key = format!("panel-duration-{i}");
                let duration = elements
                    .iter()
                    .find(|e| e.key.as_ref() == Some(&duration_key))
                    .unwrap();
                let label = elements
                    .iter()
                    .find(|e| {
                        e.text.is_some() && e.path.starts_with(&format!("{}/", duration.path))
                    })
                    .unwrap();
                edges.push(label.rect[0] + label.rect[2]);
            }
            assert_eq!(tracks, vec![tracks[0]; 3]);
            assert!(edges.iter().all(|edge| (*edge - edges[0]).abs() < 0.01));
            for (name, ink) in [
                ("X", Color::WHITE),
                ("G", Color::BLACK),
                ("T", Color::WHITE),
            ] {
                let monogram = elements
                    .iter()
                    .find(|e| e.text.as_deref() == Some(name))
                    .unwrap();
                assert_eq!(monogram.text_color, Some(ink));
            }
            assert_ne!(
                rt.state.palette.fill(&rt.state.rows[0].key, dark),
                rt.state.palette.fill(&rt.state.rows[1].key, dark)
            );
        }
    }
}
