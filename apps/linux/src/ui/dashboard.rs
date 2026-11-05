use super::{
    icons::{self, IconSampler},
    treemap::{self, Drill, Treemap},
};
use crate::{
    app::{self, AppState},
    colors::{self, Lch, Rgb},
    day, format, layout,
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use spentday_core::{DayDetail, DayItem, ItemKind, Session};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub struct Dashboard {
    pub window: adw::ApplicationWindow,
    state: Rc<AppState>,
    toasts: adw::ToastOverlay,
    date: gtk::Label,
    next: gtk::Button,
    total: gtk::Label,
    sentence: gtk::Label,
    crumbs: gtk::Box,
    details: gtk::Expander,
    details_panel: gtk::ScrolledWindow,
    detail_rows: gtk::Box,
    stack: gtk::Stack,
    strip: gtk::DrawingArea,
    selected: RefCell<glib::DateTime>,
    last_today: Cell<i64>,
    detail: RefCell<Option<DayDetail>>,
    usual: Cell<Option<i64>>,
    drill: RefCell<Drill>,
    hovered: RefCell<Option<layout::MapItem>>,
    map: RefCell<Option<Treemap>>,
    colors: RefCell<HashMap<String, Rgb>>,
    icons: RefCell<IconSampler>,
    icon_paths: RefCell<HashMap<String, Vec<String>>>,
    generation: Cell<u64>,
}
impl Dashboard {
    pub fn new(app: &adw::Application, state: Rc<AppState>) -> Rc<Self> {
        let previous = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Previous day (Ctrl+Left)")
            .build();
        previous.add_css_class("flat");
        previous.update_property(&[gtk::accessible::Property::Label("Previous day")]);
        let next = gtk::Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text("Next day (Ctrl+Right)")
            .build();
        next.add_css_class("flat");
        next.update_property(&[gtk::accessible::Property::Label("Next day")]);
        let date = gtk::Label::new(None);
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        navigation.append(&previous);
        navigation.append(&date);
        navigation.append(&next);
        let menu = gio::Menu::new();
        for (label, action) in [
            ("Sync now", "app.sync"),
            ("Preferences", "app.preferences"),
            ("About Spent Day", "app.about"),
            ("Stop tracking and quit", "app.quit"),
        ] {
            menu.append(Some(label), Some(action));
        }
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .primary(true)
            .tooltip_text("Main menu")
            .build();
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
        header.pack_start(&navigation);
        header.pack_end(&menu_button);
        let crumbs = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        crumbs.set_visible(false);
        let total = gtk::Label::builder().xalign(0.).label("0m").build();
        total.add_css_class("spentday-total");
        let sentence = gtk::Label::builder().xalign(0.).wrap(true).build();
        sentence.add_css_class("dim-label");
        let detail_rows = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&detail_rows)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .visible(false)
            .max_content_height(170)
            .propagate_natural_height(true)
            .build();
        let details = gtk::Expander::builder()
            .label("Details")
            .visible(false)
            .build();
        details.connect_expanded_notify(glib::clone!(
            #[weak]
            scroller,
            move |expander| scroller.set_visible(expander.is_expanded() && expander.is_visible())
        ));
        let sentence_line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        sentence_line.append(&sentence);
        details.set_valign(gtk::Align::Start);
        sentence_line.append(&details);
        let stack = gtk::Stack::builder()
            .vexpand(true)
            .hexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(250)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .build();
        let empty = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .build();
        let title = gtk::Label::new(Some("No screen time recorded"));
        title.add_css_class("heading");
        empty.append(&title);
        let message = gtk::Label::new(Some("Nothing was recorded for this day."));
        message.add_css_class("dim-label");
        empty.append(&message);
        stack.add_named(&empty, Some("empty"));
        let loading = gtk::Spinner::builder()
            .spinning(true)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        stack.add_named(&loading, Some("loading"));
        stack.set_visible_child_name("loading");
        let strip = gtk::DrawingArea::builder()
            .content_height(30)
            .margin_top(16)
            .visible(false)
            .build();
        strip.update_property(&[gtk::accessible::Property::Label(
            "Screen time across the day, midnight to midnight",
        )]);
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(36)
            .margin_end(36)
            .margin_top(18)
            .margin_bottom(26)
            .spacing(6)
            .build();
        content.append(&crumbs);
        content.append(&total);
        content.append(&sentence_line);
        content.append(&scroller);
        stack.set_margin_top(12);
        content.append(&stack);
        content.append(&strip);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&content));
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&toasts));
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(if state.demo {
                "Spent Day (Demo Data)"
            } else {
                "Spent Day"
            })
            .default_width(1280)
            .default_height(800)
            .width_request(360)
            .height_request(420)
            .content(&view)
            .build();
        window.add_css_class("calm-day");
        if adw::StyleManager::default().is_dark() {
            window.add_css_class("dark");
        }
        let now = glib::DateTime::now_local().unwrap();
        let selected = if state.demo {
            glib::DateTime::from_local(2026, 10, 28, 0, 0, 0.).unwrap()
        } else {
            day::midnight(&now)
        };
        let dashboard = Rc::new(Self {
            window,
            state,
            toasts,
            date,
            next,
            total,
            sentence,
            crumbs,
            details,
            details_panel: scroller,
            detail_rows,
            stack,
            strip,
            selected: RefCell::new(selected),
            last_today: Cell::new(day::query(&now).start_ms),
            detail: RefCell::new(None),
            usual: Cell::new(None),
            drill: RefCell::new(Drill::Day),
            hovered: RefCell::new(None),
            map: RefCell::new(None),
            colors: RefCell::new(HashMap::new()),
            icons: RefCell::new(IconSampler::new()),
            icon_paths: RefCell::new(HashMap::new()),
            generation: Cell::new(0),
        });
        previous.connect_clicked(glib::clone!(
            #[weak]
            dashboard,
            move |_| dashboard.step(-1)
        ));
        dashboard.next.connect_clicked(glib::clone!(
            #[weak]
            dashboard,
            move |_| dashboard.step(1)
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Bubble);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            dashboard,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                    match key {
                        gdk::Key::Left => dashboard.step(-1),
                        gdk::Key::Right => dashboard.step(1),
                        _ => return glib::Propagation::Proceed,
                    }
                } else if key == gdk::Key::Escape {
                    if dashboard
                        .map
                        .borrow()
                        .as_ref()
                        .is_some_and(|m| m.dismiss_hover())
                    {
                        return glib::Propagation::Stop;
                    }
                    let parent = match &*dashboard.drill.borrow() {
                        Drill::Day => return glib::Propagation::Proceed,
                        Drill::Device(_) => Drill::Day,
                        Drill::Item(id, _) => Drill::Device(id.clone()),
                    };
                    dashboard.navigate(parent);
                } else {
                    return glib::Propagation::Proceed;
                }
                glib::Propagation::Stop
            }
        ));
        dashboard.window.add_controller(keys);
        dashboard.strip.set_draw_func(glib::clone!(
            #[weak]
            dashboard,
            move |area, cr, w, _| {
                if let Some(detail) = dashboard.detail.borrow().as_ref() {
                    let hovered = dashboard.hovered.borrow();
                    let selected = match &*dashboard.drill.borrow() {
                        Drill::Item(device, key) => detail
                            .devices
                            .iter()
                            .find(|d| d.device.id == *device)
                            .and_then(|d| {
                                layout::map_items(d, detail.wall_ms)
                                    .into_iter()
                                    .find(|i| i.item.key == *key)
                            }),
                        _ => None,
                    };
                    treemap::draw_strip(
                        area,
                        cr,
                        w as f64,
                        detail,
                        &dashboard.colors.borrow(),
                        hovered.as_ref().or(selected.as_ref()),
                        None,
                    );
                }
            }
        ));
        let style = adw::StyleManager::default();
        let dark_handler = style.connect_dark_notify(glib::clone!(
            #[weak]
            dashboard,
            move |style| {
                if style.is_dark() {
                    dashboard.window.add_css_class("dark");
                } else {
                    dashboard.window.remove_css_class("dark");
                }
                dashboard.render(false);
            }
        ));
        let dark_handler = RefCell::new(Some(dark_handler));
        dashboard.window.connect_destroy(move |_| {
            if let Some(handler) = dark_handler.take() {
                style.disconnect(handler);
            }
        });
        glib::timeout_add_seconds_local(
            30,
            glib::clone!(
                #[weak]
                dashboard,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    dashboard.refresh();
                    glib::ControlFlow::Continue
                }
            ),
        );
        dashboard.refresh();
        dashboard
    }
    fn step(self: &Rc<Self>, offset: i32) {
        let next = self.selected.borrow().add_days(offset).unwrap();
        if next.to_unix() > day::midnight(&glib::DateTime::now_local().unwrap()).to_unix() {
            return;
        }
        *self.selected.borrow_mut() = next;
        *self.drill.borrow_mut() = Drill::Day;
        self.detail.take();
        self.usual.set(None);
        self.details.set_expanded(false);
        if let Some(map) = self.map.borrow().as_ref() {
            map.clear_hover();
        }
        self.stack.set_visible_child_name("loading");
        self.strip.set_visible(false);
        self.sentence.set_label("");
        self.details.set_visible(false);
        self.crumbs.set_visible(false);
        self.total.set_label("");
        self.refresh();
    }
    pub fn refresh(self: &Rc<Self>) {
        let now = glib::DateTime::now_local().unwrap();
        let today = day::query(&now).start_ms;
        if self.selected.borrow().to_unix() * 1000 == self.last_today.get()
            && today != self.last_today.get()
        {
            *self.selected.borrow_mut() = day::midnight(&now);
            *self.drill.borrow_mut() = Drill::Day;
            self.detail.take();
        }
        self.last_today.set(today);
        self.date
            .set_label(&self.selected.borrow().format("%A, %-d %B").unwrap());
        self.next
            .set_sensitive(self.selected.borrow().to_unix() * 1000 < today);
        let query = day::query(&self.selected.borrow());
        let history = day::usual_queries(&self.selected.borrow(), &now);
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let store = self.state.store.clone();
        let demo = self.state.demo;
        app::spawn(
            move || {
                let detail = store.day(query)?;
                let days = history
                    .into_iter()
                    .map(|q| store.day(q))
                    .collect::<Result<Vec<_>, _>>()?;
                let icons = if demo {
                    HashMap::new()
                } else {
                    icons::resolve(&detail)
                };
                Ok::<_, spentday_core::SpentDayError>((detail, day::usual_total(&days), icons))
            },
            glib::clone!(
                #[weak(rename_to=this)]
                self,
                move |result| {
                    if this.generation.get() != generation {
                        return;
                    }
                    match result {
                        Ok((detail, usual, icons)) => {
                            *this.icon_paths.borrow_mut() = icons;
                            let changed = this.detail.borrow().as_ref() != Some(&detail)
                                || this.usual.get() != usual;
                            *this.detail.borrow_mut() = Some(detail);
                            this.usual.set(usual);
                            if changed {
                                this.render(false);
                            }
                        }
                        Err(e) => {
                            this.toast(&format::error("could not load activity", &e));
                            if this.detail.borrow().is_none() {
                                this.stack.set_visible_child_name("empty");
                                this.total.set_label("0m");
                            }
                        }
                    }
                }
            ),
        );
    }
    fn navigate(self: &Rc<Self>, drill: Drill) {
        if *self.drill.borrow() == drill {
            return;
        }
        *self.drill.borrow_mut() = drill;
        self.render(true);
    }
    fn render(self: &Rc<Self>, animate: bool) {
        let Some(detail) = self.detail.borrow().clone() else {
            return;
        };
        let drill = self.drill.borrow().clone();
        let device = match &drill {
            Drill::Day => None,
            Drill::Device(id) | Drill::Item(id, _) => {
                detail.devices.iter().find(|d| d.device.id == *id)
            }
        };
        let item = match &drill {
            Drill::Item(_, key) => device.and_then(|d| d.items.iter().find(|i| i.key == *key)),
            _ => None,
        };
        if device.is_none() && drill != Drill::Day
            || matches!(drill, Drill::Item(..)) && item.is_none()
        {
            *self.drill.borrow_mut() = Drill::Day;
            self.render(animate);
            return;
        }
        while let Some(child) = self.crumbs.first_child() {
            self.crumbs.remove(&child);
        }
        self.crumbs.set_visible(device.is_some());
        if let Some(device) = device {
            let back = gtk::Button::with_label("All devices");
            back.add_css_class("crumb");
            back.connect_clicked(glib::clone!(
                #[weak(rename_to=this)]
                self,
                move |_| this.navigate(Drill::Day)
            ));
            self.crumbs.append(&back);
            self.crumbs.append(&gtk::Label::new(Some("›")));
            let back = gtk::Button::with_label(&device.device.name);
            back.add_css_class("crumb");
            let id = device.device.id.clone();
            back.connect_clicked(glib::clone!(
                #[weak(rename_to=this)]
                self,
                move |_| this.navigate(Drill::Device(id.clone()))
            ));
            self.crumbs.append(&back);
            if let Some(item) = item {
                self.crumbs.append(&gtk::Label::new(Some("›")));
                self.crumbs.append(&gtk::Label::new(Some(&item.name)));
            }
        }
        let total = item.map_or_else(
            || device.map_or(detail.wall_ms, |d| d.total_ms),
            |i| i.total_ms,
        );
        self.total.set_label(&format::duration(total));
        self.total
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{} of screen time",
                day::accessible_duration(total)
            ))]);
        let sessions: Vec<Session> = if let Some(item) = item {
            item.sessions.clone()
        } else if let Some(device) = device {
            device
                .items
                .iter()
                .flat_map(|i| i.sessions.clone())
                .collect()
        } else {
            detail
                .devices
                .iter()
                .flat_map(|d| d.items.iter().flat_map(|i| i.sessions.clone()))
                .collect()
        };
        let first = sessions.iter().map(|s| s.start_ms).min();
        let last = sessions.iter().map(|s| s.end_ms).max();
        let sentence = if let (Some(first), Some(last)) = (first, last) {
            let from = day::clock(first, detail.end_ms);
            let to = day::clock(last, detail.end_ms);
            if let Some(device) = device {
                if let Some(item) = item {
                    format!(
                        "{} on {}, {} {}, {from} to {to}.",
                        item.name,
                        device.device.name,
                        item.sessions.len(),
                        if item.sessions.len() == 1 {
                            "session"
                        } else {
                            "sessions"
                        }
                    )
                } else {
                    format!(
                        "{}, {} {}, {from} to {to}.",
                        device.device.name,
                        device.items.len(),
                        if device.items.len() == 1 {
                            "app or site"
                        } else {
                            "apps and sites"
                        }
                    )
                }
            } else {
                let comparison = self
                    .usual
                    .get()
                    .map(|usual| {
                        let delta = detail.wall_ms - usual;
                        let weekday = self.selected.borrow().format("%A").unwrap();
                        if delta.abs() < 60_000 {
                            format!("About the same as a usual {weekday}. ")
                        } else {
                            format!(
                                "{} {} than a usual {weekday}. ",
                                format::duration(delta.abs()),
                                if delta > 0 { "more" } else { "less" }
                            )
                        }
                    })
                    .unwrap_or_default();
                format!("{comparison}First screen {from}, last {to}.")
            }
        } else {
            String::new()
        };
        self.sentence.set_label(&sentence);
        self.details
            .set_visible(device.is_none() && detail.wall_ms > 0);
        self.details_panel
            .set_visible(device.is_none() && detail.wall_ms > 0 && self.details.is_expanded());
        self.show_details(&detail);
        let dark = adw::StyleManager::default().is_dark();
        let mut ranked: HashMap<String, (i64, DayItem)> = HashMap::new();
        for item in detail.devices.iter().flat_map(|d| &d.items) {
            let entry = ranked.entry(item.key.clone()).or_insert((0, item.clone()));
            entry.0 += item.total_ms;
        }
        let mut ranked: Vec<_> = ranked.into_iter().collect();
        ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
        let seeds = ranked.into_iter().map(|(key, (_, item))| {
            let seed = if self.state.demo {
                crate::demo::color_seed(&item)
            } else if item.kind == ItemKind::Site {
                colors::hashed(&key)
            } else {
                self.icons.borrow_mut().seed(
                    &item,
                    self.icon_paths
                        .borrow()
                        .get(&item.app_key)
                        .map_or(&[], Vec::as_slice),
                    &self.window,
                )
            };
            (
                key,
                if seed.c == 0. {
                    Lch {
                        l: if dark { seed.l } else { 0.78 },
                        c: 0.,
                        h: 0.,
                    }
                } else {
                    seed.tuned(dark)
                },
            )
        });
        *self.colors.borrow_mut() = colors::separate(seeds, dark);
        self.stack.set_transition_duration(
            if animate && gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations()) {
                250
            } else {
                0
            },
        );
        if detail.wall_ms == 0 {
            self.stack.set_visible_child_name("empty");
            self.strip.set_visible(false);
            if let Some(map) = self.map.borrow().as_ref() {
                map.clear_hover();
            }
            return;
        }
        if let Some(map) = self.map.borrow().as_ref().filter(|m| m.drill() == drill) {
            map.update(detail, self.colors.borrow().clone());
            self.stack.set_visible_child(map);
            self.strip.set_visible(true);
            self.strip.queue_draw();
            return;
        }
        if let Some(map) = self.map.take() {
            map.clear_hover();
        }
        let map = Treemap::new(
            detail,
            drill,
            self.colors.borrow().clone(),
            Rc::new(glib::clone!(
                #[weak(rename_to=this)]
                self,
                move |drill| this.navigate(drill)
            )),
            Rc::new(glib::clone!(
                #[weak(rename_to=this)]
                self,
                move |hovered| {
                    *this.hovered.borrow_mut() = hovered;
                    this.strip.queue_draw();
                }
            )),
        );
        self.stack.add_child(&map);
        self.stack.set_visible_child(&map);
        *self.map.borrow_mut() = Some(map.clone());
        self.strip.set_visible(true);
        self.strip.queue_draw();
        if animate {
            map.focus_first();
        }
        let stack = self.stack.clone();
        let map = map.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_millis(260), move || {
            let Some(map) = map.upgrade() else {
                return;
            };
            if stack.visible_child().as_ref() != Some(map.upcast_ref()) {
                return;
            }
            let pages = stack.pages();
            let old: Vec<gtk::Widget> = (0..pages.n_items())
                .filter_map(|i| {
                    pages
                        .item(i)
                        .and_downcast::<gtk::StackPage>()
                        .map(|p| p.child())
                })
                .filter(|w| w.is::<Treemap>() && w != map.upcast_ref::<gtk::Widget>())
                .collect();
            for child in old {
                stack.remove(&child);
            }
        });
    }
    fn show_details(&self, detail: &DayDetail) {
        while let Some(child) = self.detail_rows.first_child() {
            self.detail_rows.remove(&child);
        }
        let mut lines = vec![format!(
            "Time on two screens at once: {}. {} of device time.",
            format::duration(detail.device_ms - detail.wall_ms),
            format::duration(detail.device_ms)
        )];
        let longest = detail
            .devices
            .iter()
            .flat_map(|d| {
                layout::union_sessions(d.items.iter().flat_map(|i| i.sessions.clone()))
                    .into_iter()
                    .map(move |s| (&d.device.name, s))
            })
            .max_by_key(|(_, s)| s.end_ms - s.start_ms);
        if let Some((name, s)) = longest {
            lines.push(format!(
                "Longest stretch: {} on {name}, {} to {}.",
                format::duration(s.end_ms - s.start_ms),
                day::clock(s.start_ms, detail.end_ms),
                day::clock(s.end_ms, detail.end_ms)
            ));
        }
        let date = self.selected.borrow();
        let late = glib::DateTime::new(
            &date.timezone(),
            date.year(),
            date.month(),
            date.day_of_month(),
            23,
            0,
            0.,
        )
        .unwrap()
        .to_unix()
            * 1000;
        let late_ms: i64 = layout::union_sessions(
            detail
                .devices
                .iter()
                .flat_map(|d| d.items.iter().flat_map(|i| i.sessions.clone())),
        )
        .iter()
        .map(|s| (s.end_ms - s.start_ms.max(late)).max(0))
        .sum();
        lines.push(format!("Time after 23:00: {}.", format::duration(late_ms)));
        for line in lines {
            self.detail_rows.append(
                &gtk::Label::builder()
                    .label(line)
                    .xalign(0.)
                    .wrap(true)
                    .build(),
            );
        }
        for device in &detail.devices {
            let title = gtk::Label::builder()
                .label(&device.device.name)
                .xalign(0.)
                .margin_top(4)
                .build();
            title.add_css_class("heading");
            self.detail_rows.append(&title);
            for item in &device.items {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row.append(
                    &gtk::Label::builder()
                        .label(&item.name)
                        .xalign(0.)
                        .hexpand(true)
                        .wrap(true)
                        .build(),
                );
                row.append(&gtk::Label::new(Some(&format::duration(item.total_ms))));
                self.detail_rows.append(&row);
            }
        }
    }
    pub fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }
}
