use super::chart::Chart;
use crate::app::{self, AppState};
use crate::desktop::Entries;
use crate::format;
use crate::range::Range;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use spentday_core::{AppUsage, DeviceInfo, DomainUsage, Summary};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

const REFRESH_SECS: u32 = 30;

pub struct Dashboard {
    pub window: adw::ApplicationWindow,
    state: Rc<AppState>,
    toasts: adw::ToastOverlay,
    ranges: adw::ToggleGroup,
    device_menu: gtk::DropDown,
    device_names: gtk::StringList,
    /// Parallel to `device_names`; `None` is "All Devices".
    device_ids: RefCell<Vec<Option<String>>>,
    stack: gtk::Stack,
    empty: adw::StatusPage,
    caption: gtk::Label,
    total: gtk::Label,
    detail: gtk::Label,
    chart: Chart,
    apps: gtk::ListBox,
    sites: gtk::ListBox,
    sites_group: adw::PreferencesGroup,
    /// Bumped on every refresh so a slow, stale result never overwrites a newer one.
    generation: Cell<u64>,
}

impl Dashboard {
    pub fn new(app: &adw::Application, state: Rc<AppState>) -> Rc<Dashboard> {
        let ranges = adw::ToggleGroup::new();
        for range in Range::ALL {
            ranges.add(adw::Toggle::builder().label(range.label()).name(range.id()).build());
        }
        ranges.set_active_name(Some(Range::Today.id()));

        let device_names = gtk::StringList::new(&["All Devices"]);
        let device_menu = gtk::DropDown::builder().model(&device_names).tooltip_text("Device").build();
        device_menu.add_css_class("flat");

        let menu = gio::Menu::new();
        let section = gio::Menu::new();
        section.append(Some("Sync Now"), Some("app.sync"));
        section.append(Some("Preferences"), Some("app.preferences"));
        section.append(Some("About Spent Day"), Some("app.about"));
        menu.append_section(None, &section);
        let quit = gio::Menu::new();
        quit.append(Some("Stop Tracking and Quit"), Some("app.quit"));
        menu.append_section(None, &quit);
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .primary(true)
            .tooltip_text("Main Menu")
            .build();

        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&ranges));
        header.pack_start(&device_menu);
        header.pack_end(&menu_button);

        let caption = gtk::Label::builder().xalign(0.0).build();
        caption.add_css_class("heading");
        caption.add_css_class("dim-label");
        let total = gtk::Label::builder().xalign(0.0).build();
        total.add_css_class("spentday-total");
        let detail = gtk::Label::builder().xalign(0.0).wrap(true).build();
        detail.add_css_class("dim-label");
        let hero = gtk::Box::new(gtk::Orientation::Vertical, 2);
        hero.append(&caption);
        hero.append(&total);
        hero.append(&detail);

        let chart = Chart::new();
        let chart_card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        chart_card.add_css_class("card");
        chart_card.add_css_class("spentday-chart");
        chart_card.append(&chart.area);

        let list = || {
            let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).build();
            list.add_css_class("boxed-list");
            list
        };
        let apps = list();
        let apps_group = adw::PreferencesGroup::builder().title("Apps").build();
        apps_group.add(&apps);
        let sites = list();
        let sites_group = adw::PreferencesGroup::builder().title("Sites").build();
        sites_group.add(&sites);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .margin_top(24)
            .margin_bottom(32)
            .margin_start(12)
            .margin_end(12)
            .build();
        for child in [hero.upcast_ref::<gtk::Widget>(), chart_card.upcast_ref(), apps_group.upcast_ref(), sites_group.upcast_ref()] {
            content.append(child);
        }
        let clamp = adw::Clamp::builder().maximum_size(720).tightening_threshold(560).child(&content).build();
        let scroller = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&clamp).build();

        let empty = adw::StatusPage::builder().icon_name("preferences-system-time-symbolic").build();
        let stack = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).build();
        stack.add_named(&scroller, Some("dashboard"));
        stack.add_named(&empty, Some("empty"));

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&stack));
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&toasts));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(if state.demo { "Spent Day (Demo Data)" } else { "Spent Day" })
            .default_width(760)
            .default_height(860)
            .width_request(360)
            .height_request(420)
            .content(&view)
            .build();

        let dashboard = Rc::new(Dashboard {
            window,
            state,
            toasts,
            ranges,
            device_menu,
            device_names,
            device_ids: RefCell::new(vec![None]),
            stack,
            empty,
            caption,
            total,
            detail,
            chart,
            apps,
            sites,
            sites_group,
            generation: Cell::new(0),
        });
        let weak = Rc::downgrade(&dashboard);
        let refresh = move || {
            if let Some(dashboard) = weak.upgrade() {
                dashboard.refresh();
            }
        };
        dashboard.ranges.connect_active_name_notify({
            let refresh = refresh.clone();
            move |_| refresh()
        });
        dashboard.device_menu.connect_selected_notify({
            let refresh = refresh.clone();
            move |_| refresh()
        });
        let window = dashboard.window.downgrade();
        glib::timeout_add_seconds_local(REFRESH_SECS, move || {
            let Some(window) = window.upgrade() else { return glib::ControlFlow::Break };
            if window.is_visible() {
                refresh();
            }
            glib::ControlFlow::Continue
        });
        dashboard.refresh();
        dashboard
    }

    fn range(&self) -> Range {
        Range::from_id(self.ranges.active_name().as_deref().unwrap_or_default())
    }

    fn device(&self) -> Option<String> {
        self.device_ids.borrow().get(self.device_menu.selected() as usize).cloned().flatten()
    }

    pub fn refresh(self: &Rc<Self>) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let range = self.range();
        let now = glib::DateTime::now_local().expect("local time");
        let (start, _) = range.bounds(&now);
        let query = range.query(&now, self.device());
        let store = self.state.store.clone();
        let weak = Rc::downgrade(self);
        app::spawn(
            move || (store.summary(query), store.devices()),
            move |(summary, devices)| {
                let Some(this) = weak.upgrade() else { return };
                if this.generation.get() != generation {
                    return;
                }
                if let Ok(devices) = &devices {
                    this.set_devices(devices);
                }
                match summary {
                    Ok(summary) => this.show(range, &start, &summary),
                    Err(e) => this.toast(&format!("Could not load activity: {e}")),
                }
            },
        );
    }

    fn set_devices(&self, devices: &[DeviceInfo]) {
        let ids: Vec<Option<String>> = std::iter::once(None).chain(devices.iter().map(|d| Some(d.id.clone()))).collect();
        if *self.device_ids.borrow() == ids {
            return;
        }
        let selected = self.device();
        let names: Vec<String> = devices
            .iter()
            .enumerate()
            .map(|(i, d)| if i == 0 { format!("{} (This Device)", d.name) } else { d.name.clone() })
            .collect();
        // Replacing the model resets the selection, and the notify handler refreshes with
        // whatever is selected, so the ids must be in place first.
        *self.device_ids.borrow_mut() = ids.clone();
        let names: Vec<&str> = std::iter::once("All Devices").chain(names.iter().map(String::as_str)).collect();
        self.device_names.splice(0, self.device_names.n_items(), &names);
        let index = ids.iter().position(|id| *id == selected).unwrap_or(0);
        self.device_menu.set_selected(index as u32);
        self.device_menu.set_visible(devices.len() > 1);
    }

    fn show(&self, range: Range, start: &glib::DateTime, summary: &Summary) {
        let device_name = self.device().and_then(|id| {
            summary.by_device.iter().find(|d| d.device.id == id).map(|d| d.device.name.clone())
        });
        if summary.total_ms == 0 {
            self.empty.set_title(if range == Range::Today { "No Screen Time Yet" } else { "Nothing Recorded" });
            self.empty.set_description(Some(&match (range, &device_name) {
                (Range::Today, None) => "Spent Day records the app in front of you while it runs. Check back in a few minutes.".to_owned(),
                (_, Some(name)) => format!("Nothing was recorded on {name} in this period."),
                (_, None) => "Nothing was recorded in this period.".to_owned(),
            }));
            self.stack.set_visible_child_name("empty");
            return;
        }
        self.stack.set_visible_child_name("dashboard");

        self.caption.set_label(range.label());
        self.total.set_label(&format::duration(summary.total_ms));
        let mut detail = Vec::new();
        match (&device_name, summary.by_device.iter().filter(|d| d.total_ms > 0).count()) {
            (Some(name), _) => detail.push(format!("on {name}")),
            (None, n) if n > 1 => detail.push(format!("across {n} devices")),
            _ => {}
        }
        if let Some((i, _)) = summary.buckets.iter().enumerate().filter(|(_, ms)| **ms > 0).max_by_key(|(_, ms)| **ms) {
            detail.push(if range == Range::Week {
                let day = start.add_days(i as i32).and_then(|d| d.format("%A")).map(|s| s.to_string()).unwrap_or_default();
                format!("busiest on {day}")
            } else {
                format!("busiest around {i:02}:00")
            });
        }
        self.detail.set_label(&capitalize(&detail.join(" · ")));
        self.chart.set(range, start, summary.buckets.clone());

        self.apps.remove_all();
        let top = summary.apps.first().map_or(1, |a| a.total_ms);
        let apps: Vec<&AppUsage> = summary.apps.iter().filter(|a| a.total_ms > 0).collect();
        let any_expander = apps.iter().any(|a| a.domains.iter().any(|d| d.total_ms > 0));
        for app in apps {
            self.apps.append(&app_row(app, top, any_expander));
        }
        self.sites.remove_all();
        let top = summary.sites.first().map_or(1, |s| s.total_ms);
        for site in summary.sites.iter().filter(|s| s.total_ms > 0) {
            self.sites.append(&site_row(site, top));
        }
        self.sites_group.set_visible(summary.sites.iter().any(|s| s.total_ms > 0));
    }

    pub fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// `align` reserves the width of an expander arrow, so bars line up with browser rows.
fn app_row(app: &AppUsage, top: i64, align: bool) -> gtk::Widget {
    let domains: Vec<&DomainUsage> = app.domains.iter().filter(|d| d.total_ms > 0).collect();
    let sites = domains.iter().filter(|d| !d.domain.is_empty()).count();
    if sites == 0 {
        let row = adw::ActionRow::builder().title(&app.app_name).use_markup(false).title_lines(1).build();
        row.add_prefix(&app_icon(app));
        row.add_suffix(&usage(app.total_ms, top));
        if align {
            row.add_suffix(&gtk::Box::builder().width_request(20).build());
        }
        return row.upcast();
    }
    let sites = if sites == 1 { "1 site".to_owned() } else { format!("{sites} sites") };
    let row = adw::ExpanderRow::builder().title(&app.app_name).subtitle(&sites).use_markup(false).title_lines(1).build();
    row.add_prefix(&app_icon(app));
    row.add_suffix(&usage(app.total_ms, top));
    for domain in domains {
        row.add_row(&site_row(domain, app.total_ms));
    }
    row.upcast()
}

/// An empty domain is the browser's time with no site recorded.
fn site_row(site: &DomainUsage, top: i64) -> adw::ActionRow {
    let title = if site.domain.is_empty() { "Other" } else { &site.domain };
    let row = adw::ActionRow::builder().title(title).use_markup(false).title_lines(1).build();
    row.add_suffix(&usage(site.total_ms, top));
    row
}

thread_local! {
    static ENTRIES: RefCell<Entries> = RefCell::new(Entries::new());
}

/// The icon of the first platform id behind the row that has one, by theme icon name or by its
/// `.desktop` entry, otherwise coloured initials.
fn app_icon(app: &AppUsage) -> gtk::Widget {
    let theme = gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d));
    let has = |name: &str| theme.as_ref().is_some_and(|t| t.has_icon(name));
    let ids = app.app_ids.iter().map(String::as_str).chain([app.app_key.as_str()]);
    let image = ENTRIES.with_borrow_mut(|entries| {
        ids.into_iter().find_map(|id| {
            let lower = id.to_lowercase();
            if let Some(name) = [id, lower.as_str()].into_iter().find(|name| has(name)) {
                return Some(gtk::Image::from_icon_name(name));
            }
            match entries.info(id).icon.as_deref() {
                Some(path) if path.starts_with('/') => Path::new(path).is_file().then(|| gtk::Image::from_file(path)),
                Some(name) if has(name) => Some(gtk::Image::from_icon_name(name)),
                _ => None,
            }
        })
    });
    match image {
        Some(image) => {
            image.set_pixel_size(32);
            image.add_css_class("lowres-icon");
            image.upcast()
        }
        None => adw::Avatar::builder().text(&app.app_name).show_initials(true).size(32).build().upcast(),
    }
}

/// A bar scaled to `top`, then the duration.
fn usage(ms: i64, top: i64) -> gtk::Box {
    let bar = gtk::ProgressBar::builder()
        .fraction((ms as f64 / top.max(1) as f64).clamp(0.0, 1.0))
        .valign(gtk::Align::Center)
        .width_request(120)
        .build();
    bar.add_css_class("spentday-bar");
    let label = gtk::Label::builder().label(format::duration(ms)).width_chars(7).xalign(1.0).build();
    label.add_css_class("numeric");
    let suffix = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    suffix.append(&bar);
    suffix.append(&label);
    suffix
}
