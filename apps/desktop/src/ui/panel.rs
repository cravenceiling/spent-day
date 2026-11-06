//! The menu bar's compact summary. Linux registers it for scene renders too.

use crate::app::Shared;
use crate::platform;
use crate::sync::{self, SyncView, Trigger};
use crate::ui::{bg, font, kit, settings};
use rune::{Cx, El, ImageCache, Key, KeyEvent, Tx, col, el, row, text};
use spentday_core::SummaryQuery;
use std::sync::Arc;

pub const SIZE: (f32, f32) = (264.0, 350.0);

pub struct Panel {
    pub shared: Arc<Shared>,
    pub total_ms: i64,
    pub rows: Vec<PanelRow>,
    pub sync: SyncView,
    pub armed: bool,
    pub generation: u64,
    images: Option<Arc<ImageCache>>,
}

pub struct PanelRow {
    pub name: String,
    pub app_ids: Vec<String>,
    pub total_ms: i64,
    pub icon: Option<rune::Image>,
}

impl Panel {
    pub fn new(shared: Arc<Shared>) -> Panel {
        Panel {
            shared,
            total_ms: 0,
            rows: Vec::new(),
            sync: SyncView::default(),
            armed: false,
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
                let wall_ms = shared
                    .store
                    .day(q.clone())
                    .map_err(|e| crate::format::error("load today's total", &e))?
                    .wall_ms;
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
                            name: app.app_name,
                            app_ids: app.app_ids,
                            total_ms: app.total_ms,
                            icon,
                        }
                    })
                    .collect();
                Ok::<_, String>((wall_ms, rows))
            },
            move |p, result, _| {
                if generation != p.generation {
                    return false;
                }
                if let Ok((total_ms, rows)) = result {
                    p.total_ms = total_ms;
                    p.rows = rows;
                    return true;
                }
                false
            },
        );
        false
    }

    pub fn on_focus(&mut self, focused: bool, tx: &mut Tx) {
        if focused {
            self.armed = true;
            self.refresh(tx);
        } else if self.armed {
            self.armed = false;
            tx.close_window();
        }
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

fn monogram(name: &str) -> El {
    let hash = name
        .chars()
        .fold(5381u32, |n, c| n.wrapping_mul(33).wrapping_add(c as u32));
    let colors = [
        "#1477E8", "#4756D8", "#9346CF", "#DC3777", "#C96016", "#058A91", "#258644", "#128667",
        "#127FAD", "#8C6239",
    ];
    el().size(18.0, 18.0).center().child(
        el().size(15.12, 15.12)
            .radius(4.32)
            .fill(rune::color::hex(colors[hash as usize % colors.len()]).unwrap())
            .center()
            .child(
                text(
                    name.chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_else(|| "?".into()),
                )
                .font(rune::TextStyle::new(8.3, 650.0))
                .color(rune::Color::WHITE),
            ),
    )
}

pub fn view(s: &Panel, cx: &mut Cx<Panel>) -> El {
    let dark = cx.system_dark();
    let k = kit(dark);
    let top = s.rows.first().map_or(1, |r| r.total_ms.max(1));
    let apps = s.rows.iter().take(3).enumerate().map(|(i, app)| {
        let icon = match &app.icon {
            Some(icon) if icon.is_ready() && icon.error().is_none() => {
                el().size(18.0, 18.0).pixels(icon.pixels())
            }
            _ => monogram(&app.name),
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
                        el().w_full().h(4.0).radius(2.0).fill(k.c.layer_03).child(
                            el().w_frac((app.total_ms.max(0) as f32 / top as f32).min(1.0))
                                .min_w(3.0)
                                .h(4.0)
                                .radius(2.0)
                                .fill(k.c.text_accent),
                        ),
                    ),
            )
            .child(
                text(crate::format::duration(app.total_ms))
                    .font(font::LABEL)
                    .color(k.c.text_muted),
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
    if !configured || s.sync.running {
        sync_row = sync_row.dim(0.45);
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
