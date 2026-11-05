use crate::{
    colors::{self, Lch, Rgb},
    day, format,
    layout::{self, MapItem, Rect},
};
use adw::prelude::*;
use gtk::{cairo, gdk, glib, graphene, pango, subclass::prelude::*};
use spentday_core::DayDetail;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Drill {
    #[default]
    Day,
    Device(String),
    Item(String, String),
}
type Open = Rc<dyn Fn(Drill)>;
type Hover = Rc<dyn Fn(Option<MapItem>)>;

struct Child {
    button: gtk::Button,
    id: String,
    item: Option<Rc<RefCell<MapItem>>>,
}
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Treemap {
        pub(super) detail: RefCell<Option<DayDetail>>,
        pub(super) drill: RefCell<Drill>,
        pub(super) colors: RefCell<HashMap<String, Rgb>>,
        pub(super) children: RefCell<Vec<Child>>,
        pub(super) open: RefCell<Option<Open>>,
        pub(super) hover: RefCell<Option<Hover>>,
        pub(super) hovered: RefCell<Option<MapItem>>,
        pub(super) popover: RefCell<Option<gtk::Popover>>,
        pub(super) hover_epoch: Cell<u64>,
        pub(super) focus_pending: Cell<bool>,
        pub(super) refresh_popover: Cell<bool>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Treemap {
        const NAME: &'static str = "SpentDayTreemap";
        type Type = super::Treemap;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Treemap {
        fn dispose(&self) {
            self.hover_epoch.set(self.hover_epoch.get() + 1);
            if let Some(pop) = self.popover.take() {
                pop.unparent();
            }
            for child in self.children.take() {
                child.button.unparent();
            }
        }
    }
    impl WidgetImpl for Treemap {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }
        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let Some(detail) = self.detail.borrow().clone() else {
                return;
            };
            let (groups, tiles) = cells(
                &detail,
                &self.drill.borrow(),
                Rect::new(0., 0., width as f64, height as f64),
            );
            self.obj().sync_buttons(&detail, &groups, &tiles);
            let rects: HashMap<_, _> = groups
                .into_iter()
                .chain(tiles.into_iter().map(|(item, rect)| (item.id, rect)))
                .collect();
            for child in self.children.borrow().iter() {
                let Some(rect) = rects.get(&child.id).filter(|r| r.w > 0. && r.h > 0.) else {
                    child.button.set_child_visible(false);
                    continue;
                };
                child.button.set_child_visible(true);
                let x = rect.x.round() as i32;
                let y = rect.y.round() as i32;
                let w = ((rect.x + rect.w).round() as i32 - x).max(0);
                let h = ((rect.y + rect.h).round() as i32 - y).max(0);
                child.button.allocate(
                    w,
                    h,
                    -1,
                    Some(
                        gtk::gsk::Transform::new()
                            .translate(&graphene::Point::new(x as f32, y as f32)),
                    ),
                );
            }
            if self.focus_pending.replace(false) {
                self.obj().focus_first();
            }
            if self.refresh_popover.replace(false) && self.popover.borrow().is_some() {
                let hovered = self.hovered.borrow().clone();
                if let Some(cell) = hovered {
                    let button = self
                        .children
                        .borrow()
                        .iter()
                        .find(|c| c.id == cell.id)
                        .map(|c| c.button.clone());
                    if let Some(button) = button {
                        self.obj().show_popover(&button, &cell);
                    }
                }
            }
            if let Some(pop) = self.popover.borrow().as_ref() {
                if let Some(button) = pop.parent() {
                    pop.set_pointing_to(Some(&gdk::Rectangle::new(
                        0,
                        0,
                        button.width(),
                        button.height(),
                    )));
                }
                pop.present();
            }
        }
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for child in self.children.borrow().iter() {
                if child.button.is_child_visible() {
                    self.obj().snapshot_child(&child.button, snapshot);
                }
            }
        }
    }
}
glib::wrapper! { pub struct Treemap(ObjectSubclass<imp::Treemap>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget; }

type Groups = Vec<(String, Rect)>;
type Tiles = Vec<(MapItem, Rect)>;
fn cells(detail: &DayDetail, drill: &Drill, bounds: Rect) -> (Groups, Tiles) {
    let device_id = match drill {
        Drill::Day => None,
        Drill::Device(d) | Drill::Item(d, _) => Some(d),
    };
    let Some(device) = detail
        .devices
        .iter()
        .find(|d| Some(&d.device.id) == device_id)
    else {
        let groups = layout::device_groups(detail, bounds);
        return (
            groups
                .iter()
                .map(|g| (g.device.device.id.clone(), g.label))
                .collect(),
            groups.into_iter().flat_map(|g| g.tiles).collect(),
        );
    };
    let bounds = bounds.inset(-4.);
    if let Drill::Item(_, key) = drill
        && let Some(item) = device.items.iter().find(|i| i.key == *key)
    {
        let mut sessions = item.sessions.clone();
        sessions.sort_by_key(|s| s.start_ms);
        let boxes = layout::slice(
            &sessions
                .iter()
                .map(|s| (s.end_ms - s.start_ms) as f64)
                .collect::<Vec<_>>(),
            bounds,
        );
        return (
            vec![],
            sessions
                .into_iter()
                .zip(boxes)
                .enumerate()
                .map(|(index, (session, r))| {
                    (
                        MapItem {
                            id: format!("{}/{}/{index}", device.device.id, item.key),
                            device_id: device.device.id.clone(),
                            item: item.clone(),
                            session: Some(session),
                            hidden: vec![],
                        },
                        r.inset(4.),
                    )
                })
                .collect(),
        );
    }
    let items = layout::map_items(device, detail.wall_ms);
    let boxes = layout::squarify(
        &items
            .iter()
            .map(|i| i.total_ms() as f64)
            .collect::<Vec<_>>(),
        bounds,
    );
    (
        vec![],
        items
            .into_iter()
            .zip(boxes)
            .map(|(i, r)| (i, r.inset(4.)))
            .collect(),
    )
}

impl Treemap {
    pub fn new(
        detail: DayDetail,
        drill: Drill,
        colors: HashMap<String, Rgb>,
        open: Open,
        hover: Hover,
    ) -> Self {
        let map: Self = glib::Object::new();
        map.set_hexpand(true);
        map.set_vexpand(true);
        map.update_property(&[gtk::accessible::Property::Label("Screen time treemap")]);
        *map.imp().detail.borrow_mut() = Some(detail);
        *map.imp().drill.borrow_mut() = drill;
        *map.imp().colors.borrow_mut() = colors;
        *map.imp().open.borrow_mut() = Some(open);
        *map.imp().hover.borrow_mut() = Some(hover);
        map
    }
    pub fn update(&self, detail: DayDetail, colors: HashMap<String, Rgb>) {
        *self.imp().detail.borrow_mut() = Some(detail);
        *self.imp().colors.borrow_mut() = colors;
        self.imp().refresh_popover.set(true);
        self.queue_allocate();
        self.queue_tiles();
    }
    pub fn drill(&self) -> Drill {
        self.imp().drill.borrow().clone()
    }
    fn sync_buttons(&self, detail: &DayDetail, groups: &Groups, tiles: &Tiles) {
        let map = self;
        for (id, _) in groups {
            let device = detail.devices.iter().find(|d| d.device.id == *id).unwrap();
            let existing = map
                .imp()
                .children
                .borrow()
                .iter()
                .find(|c| c.id == *id)
                .map(|c| c.button.clone());
            let button = existing.clone().unwrap_or_else(gtk::Button::new);
            button.add_css_class("map-group");
            button.set_overflow(gtk::Overflow::Hidden);
            let label = button
                .child()
                .and_downcast::<gtk::Label>()
                .unwrap_or_else(|| {
                    let label = gtk::Label::builder()
                        .xalign(0.)
                        .valign(gtk::Align::Start)
                        .build();
                    button.set_child(Some(&label));
                    label
                });
            let markup = format!(
                "{}  <span alpha=\"65%\">{}</span>",
                glib::markup_escape_text(&device.device.name),
                format::duration(device.total_ms)
            );
            if label.label() != markup {
                label.set_markup(&markup);
            }
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "{}, {}. Open device",
                device.device.name,
                day::accessible_duration(device.total_ms)
            ))]);
            if existing.is_none() {
                let id = id.clone();
                button.connect_clicked(glib::clone!(
                    #[weak]
                    map,
                    #[strong]
                    id,
                    move |_| map.open(Drill::Device(id.clone()))
                ));
                map.add_button(button, id, None);
            }
        }
        for (cell, _) in tiles {
            let existing = map
                .imp()
                .children
                .borrow()
                .iter()
                .find(|c| c.id == cell.id)
                .and_then(|c| c.item.clone().map(|item| (c.button.clone(), item)));
            if let Some((button, item)) = existing {
                *item.borrow_mut() = cell.clone();
                let session = cell
                    .session
                    .as_ref()
                    .map(|s| {
                        format!(
                            ", {} to {}",
                            day::clock(s.start_ms, detail.end_ms),
                            day::clock(s.end_ms, detail.end_ms)
                        )
                    })
                    .unwrap_or_default();
                button.update_property(&[
                    gtk::accessible::Property::Label(&format!(
                        "{}, {}{session}",
                        cell.item.name,
                        day::accessible_duration(cell.total_ms())
                    )),
                    gtk::accessible::Property::Description(if cell.session.is_some() {
                        "Session"
                    } else if cell.hidden.is_empty() {
                        "Opens sessions"
                    } else {
                        "Shows hidden items"
                    }),
                ]);
                continue;
            }
            let cell = Rc::new(RefCell::new(cell.clone()));
            let button = gtk::Button::new();
            button.add_css_class("map-tile");
            let area = gtk::DrawingArea::new();
            area.set_hexpand(true);
            area.set_vexpand(true);
            area.set_draw_func(glib::clone!(
                #[weak]
                map,
                #[strong]
                cell,
                move |area, cr, w, h| {
                    let cell = cell.borrow();
                    let colors = map.imp().colors.borrow();
                    let mut fill = colors.get(&cell.item.key).copied().unwrap_or_else(|| {
                        colors::hashed(&cell.item.key)
                            .tuned(adw::StyleManager::default().is_dark())
                            .rgb()
                    });
                    if cell.session.is_some() {
                        let index = cell
                            .id
                            .rsplit('/')
                            .next()
                            .unwrap()
                            .parse::<usize>()
                            .unwrap();
                        let lch = fill.lch();
                        fill = Lch {
                            l: (lch.l + [0., -0.06, 0.04, -0.1, -0.02][index % 5]).clamp(0.3, 0.9),
                            ..lch
                        }
                        .rgb();
                    }
                    let ink = fill.ink();
                    let dim = map
                        .imp()
                        .hovered
                        .borrow()
                        .as_ref()
                        .is_some_and(|hot| hot.id != cell.id);
                    let fill = if dim {
                        fill.dimmed(adw::StyleManager::default().is_dark())
                    } else {
                        fill
                    };
                    source(cr, fill, 1.);
                    rounded(cr, 0., 0., w as f64, h as f64, 10.);
                    let _ = cr.fill();
                    if area.parent().is_some_and(|button| button.has_focus()) {
                        source(cr, ink, 1.);
                        cr.set_line_width(2.);
                        rounded(
                            cr,
                            3.,
                            3.,
                            (w as f64 - 6.).max(0.),
                            (h as f64 - 6.).max(0.),
                            10.,
                        );
                        let _ = cr.stroke();
                    }
                    let end = map.imp().detail.borrow().as_ref().unwrap().end_ms;
                    let name = cell.session.as_ref().map_or_else(
                        || cell.item.name.clone(),
                        |s| {
                            format!(
                                "{} to {}",
                                day::clock(s.start_ms, end),
                                day::clock(s.end_ms, end)
                            )
                        },
                    );
                    draw_label(
                        area,
                        cr,
                        &name,
                        &format::duration(cell.total_ms()),
                        w as f64,
                        h as f64,
                        ink,
                    );
                }
            ));
            button.set_child(Some(&area));
            let current = cell.borrow();
            let session = current
                .session
                .as_ref()
                .map(|s| {
                    format!(
                        ", {} to {}",
                        day::clock(s.start_ms, detail.end_ms),
                        day::clock(s.end_ms, detail.end_ms)
                    )
                })
                .unwrap_or_default();
            button.update_property(&[
                gtk::accessible::Property::Label(&format!(
                    "{}, {}{session}",
                    current.item.name,
                    day::accessible_duration(current.total_ms())
                )),
                gtk::accessible::Property::Description(if current.session.is_some() {
                    "Session"
                } else if current.hidden.is_empty() {
                    "Opens sessions"
                } else {
                    "Shows hidden items"
                }),
            ]);
            drop(current);
            button.connect_clicked(glib::clone!(
                #[weak]
                map,
                #[strong]
                cell,
                move |button| {
                    let cell = cell.borrow().clone();
                    if !cell.hidden.is_empty() {
                        map.enter(button, &cell, true);
                    } else if cell.session.is_none() {
                        map.open(Drill::Item(cell.device_id.clone(), cell.item.key.clone()));
                    }
                }
            ));
            let motion = gtk::EventControllerMotion::new();
            motion.connect_enter(glib::clone!(
                #[weak]
                map,
                #[weak]
                button,
                #[strong]
                cell,
                move |_, _, _| {
                    map.enter(&button, &cell.borrow(), false);
                }
            ));
            motion.connect_leave(glib::clone!(
                #[weak]
                map,
                move |_| map.leave()
            ));
            button.add_controller(motion);
            let id = cell.borrow().id.clone();
            map.add_button(button, id, Some(cell));
        }
        let hovered_id = map.imp().hovered.borrow().as_ref().map(|c| c.id.clone());
        if let Some(id) = hovered_id {
            if let Some((cell, _)) = tiles
                .iter()
                .find(|(c, r)| c.id == id && r.w > 0. && r.h > 0.)
            {
                *map.imp().hovered.borrow_mut() = Some(cell.clone());
                if let Some(hover) = map.imp().hover.borrow().as_ref() {
                    hover(Some(cell.clone()));
                }
            } else {
                map.clear_hover();
            }
        }
    }
    fn add_button(&self, button: gtk::Button, id: String, item: Option<Rc<RefCell<MapItem>>>) {
        button.set_parent(self);
        button.connect_has_focus_notify(|button| {
            if let Some(area) = button.child() {
                area.queue_draw();
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to=map)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                    return glib::Propagation::Proceed;
                }
                let step = match key {
                    gdk::Key::Left | gdk::Key::Up => -1,
                    gdk::Key::Right | gdk::Key::Down => 1,
                    _ => return glib::Propagation::Proceed,
                };
                let children = map.imp().children.borrow();
                let children: Vec<_> = children
                    .iter()
                    .filter(|c| c.button.is_child_visible())
                    .collect();
                let index = children
                    .iter()
                    .position(|c| c.button.has_focus())
                    .unwrap_or(0) as i32;
                let next = (index + step).rem_euclid(children.len() as i32) as usize;
                children[next].button.grab_focus();
                glib::Propagation::Stop
            }
        ));
        button.add_controller(keys);
        self.imp()
            .children
            .borrow_mut()
            .push(Child { button, id, item });
    }
    fn open(&self, drill: Drill) {
        self.clear_hover();
        if let Some(open) = self.imp().open.borrow().as_ref() {
            open(drill);
        }
    }
    pub fn focus_first(&self) {
        if let Some(c) = self
            .imp()
            .children
            .borrow()
            .iter()
            .find(|c| c.item.is_some() && c.button.is_child_visible())
        {
            c.button.grab_focus();
        } else {
            self.imp().focus_pending.set(true);
        }
    }
    pub fn dismiss_hover(&self) -> bool {
        let active = self.imp().popover.borrow().is_some() || self.imp().hovered.borrow().is_some();
        if active {
            self.clear_hover();
        }
        active
    }
    pub fn clear_hover(&self) {
        self.imp().hover_epoch.set(self.imp().hover_epoch.get() + 1);
        self.imp().hovered.take();
        if let Some(pop) = self.imp().popover.take() {
            pop.unparent();
        }
        self.queue_tiles();
        if let Some(hover) = self.imp().hover.borrow().as_ref() {
            hover(None);
        }
    }
    fn queue_tiles(&self) {
        for c in self.imp().children.borrow().iter() {
            if let Some(area) = c.button.child() {
                area.queue_draw();
            }
        }
    }
    fn leave(&self) {
        let epoch = self.imp().hover_epoch.get() + 1;
        self.imp().hover_epoch.set(epoch);
        glib::timeout_add_local_once(
            Duration::from_millis(90),
            glib::clone!(
                #[weak(rename_to=map)]
                self,
                move || {
                    if map.imp().hover_epoch.get() == epoch {
                        map.clear_hover();
                    }
                }
            ),
        );
    }
    fn enter(&self, button: &gtk::Button, cell: &MapItem, immediate: bool) {
        let epoch = self.imp().hover_epoch.get() + 1;
        self.imp().hover_epoch.set(epoch);
        *self.imp().hovered.borrow_mut() = Some(cell.clone());
        self.queue_tiles();
        if let Some(hover) = self.imp().hover.borrow().as_ref() {
            hover(Some(cell.clone()));
        }
        if immediate || self.imp().popover.borrow().is_some() {
            self.show_popover(button, cell);
        } else {
            glib::timeout_add_local_once(
                Duration::from_millis(300),
                glib::clone!(
                    #[weak(rename_to=map)]
                    self,
                    #[weak]
                    button,
                    #[strong]
                    cell,
                    move || {
                        if map.imp().hover_epoch.get() == epoch {
                            map.show_popover(&button, &cell);
                        }
                    }
                ),
            );
        }
    }
    fn show_popover(&self, button: &gtk::Button, cell: &MapItem) {
        let detail = self.imp().detail.borrow();
        let detail = detail.as_ref().unwrap();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(8)
            .margin_end(8)
            .width_request(236)
            .build();
        let title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let swatch = gtk::DrawingArea::builder()
            .content_width(10)
            .content_height(10)
            .valign(gtk::Align::Center)
            .build();
        let color = self
            .imp()
            .colors
            .borrow()
            .get(&cell.item.key)
            .copied()
            .unwrap_or_else(|| colors::hashed(&cell.item.key).rgb());
        swatch.set_draw_func(move |_, cr, w, h| {
            source(cr, color, 1.);
            rounded(cr, 0., 0., w as f64, h as f64, 3.);
            let _ = cr.fill();
        });
        title.append(&swatch);
        let name = gtk::Label::builder()
            .label(&cell.item.name)
            .xalign(0.)
            .build();
        name.add_css_class("heading");
        title.append(&name);
        content.append(&title);
        let device = detail
            .devices
            .iter()
            .find(|d| d.device.id == cell.device_id)
            .unwrap();
        let subtitle = gtk::Label::builder()
            .label(format!(
                "{} on {}",
                format::duration(cell.total_ms()),
                device.device.name
            ))
            .xalign(0.)
            .build();
        subtitle.add_css_class("dim-label");
        content.append(&subtitle);
        let mini = gtk::DrawingArea::builder()
            .content_height(30)
            .margin_top(8)
            .margin_bottom(4)
            .build();
        mini.set_draw_func({
            let detail = detail.clone();
            let colors = self.imp().colors.borrow().clone();
            let cell = cell.clone();
            move |a, cr, w, _| draw_strip(a, cr, w as f64, &detail, &colors, None, Some(&cell))
        });
        content.append(&mini);
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let entries: Vec<_> = if cell.hidden.is_empty() {
            cell.sessions()
                .iter()
                .map(|s| {
                    (
                        format!(
                            "{} to {}",
                            day::clock(s.start_ms, detail.end_ms),
                            day::clock(s.end_ms, detail.end_ms)
                        ),
                        s.end_ms - s.start_ms,
                    )
                })
                .collect()
        } else {
            cell.hidden
                .iter()
                .map(|i| (i.name.clone(), i.total_ms))
                .collect()
        };
        for (name, ms) in &entries {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            let name = gtk::Label::builder()
                .label(name)
                .xalign(0.)
                .hexpand(true)
                .build();
            let duration = gtk::Label::new(Some(&format::duration(*ms)));
            duration.add_css_class("dim-label");
            row.append(&name);
            row.append(&duration);
            rows.append(&row);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(110)
            .propagate_natural_height(true)
            .child(&rows)
            .build();
        content.append(&scroll);
        let pop = self
            .imp()
            .popover
            .take()
            .filter(|p| {
                if p.parent().as_ref() == Some(button.upcast_ref()) {
                    true
                } else {
                    p.unparent();
                    false
                }
            })
            .unwrap_or_else(|| {
                let pop = gtk::Popover::builder()
                    .autohide(false)
                    .has_arrow(false)
                    .build();
                pop.set_parent(button);
                pop.connect_closed(glib::clone!(
                    #[weak(rename_to=map)]
                    self,
                    move |pop| {
                        let current = map.imp().popover.borrow().as_ref() == Some(pop);
                        if current {
                            map.clear_hover();
                        }
                    }
                ));
                let motion = gtk::EventControllerMotion::new();
                motion.connect_enter(glib::clone!(
                    #[weak(rename_to=map)]
                    self,
                    move |_, _, _| map.imp().hover_epoch.set(map.imp().hover_epoch.get() + 1)
                ));
                motion.connect_leave(glib::clone!(
                    #[weak(rename_to=map)]
                    self,
                    move |_| map.leave()
                ));
                pop.add_controller(motion);
                pop
            });
        pop.set_child(Some(&content));
        pop.set_pointing_to(Some(&gdk::Rectangle::new(
            0,
            0,
            button.width(),
            button.height(),
        )));
        if !pop.is_visible() {
            pop.popup();
        }
        *self.imp().popover.borrow_mut() = Some(pop);
    }
}

pub fn source(cr: &cairo::Context, color: Rgb, alpha: f64) {
    cr.set_source_rgba(color.r, color.g, color.b, alpha);
}
pub fn rounded(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 3.).min(h / 3.);
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2., 0.);
    cr.arc(x + w - r, y + h - r, r, 0., pi / 2.);
    cr.arc(x + r, y + h - r, r, pi / 2., pi);
    cr.arc(x + r, y + r, r, pi, 1.5 * pi);
    cr.close_path();
}
fn text_layout(widget: &impl IsA<gtk::Widget>, text: &str, size: f64, bold: bool) -> pango::Layout {
    let layout = widget.create_pango_layout(Some(text));
    let mut font = pango::FontDescription::new();
    if let Some(inherited) = widget.pango_context().font_description()
        && let Some(family) = inherited.family()
    {
        font.set_family(&family);
    }
    font.set_absolute_size(size * pango::SCALE as f64);
    font.set_weight(if bold {
        pango::Weight::Semibold
    } else {
        pango::Weight::Normal
    });
    layout.set_font_description(Some(&font));
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFontFeatures::new("tnum"));
    layout.set_attributes(Some(&attrs));
    layout
}
fn draw_label(
    widget: &impl IsA<gtk::Widget>,
    cr: &cairo::Context,
    name: &str,
    duration: &str,
    w: f64,
    h: f64,
    ink: Rgb,
) {
    source(cr, ink, 1.);
    let tiers = if w >= 260. && h >= 200. {
        vec![(16., 19., 15.), (13., 15., 13.), (12., 13., 12.5)]
    } else {
        vec![(13., 15., 13.), (12., 13., 12.5)]
    };
    for (pad, ns, ds) in tiers {
        let name = text_layout(widget, name, ns, true);
        let duration = text_layout(widget, duration, ds, false);
        let (nw, nh) = name.pixel_size();
        let (dw, _) = duration.pixel_size();
        if w - 2. * pad >= nw.max(dw) as f64 + 2. && h - 2. * pad >= (ns + ds) * 1.4 + 2. {
            cr.move_to(pad, pad);
            pangocairo::functions::show_layout(cr, &name);
            cr.move_to(pad, pad + nh as f64 + 2.);
            pangocairo::functions::show_layout(cr, &duration);
            return;
        }
    }
    let name = text_layout(widget, name, 13., true);
    let (nw, nh) = name.pixel_size();
    if w - 24. >= nw as f64 + 2. && h - 20. >= nh as f64 {
        cr.move_to(12., 10.);
        pangocairo::functions::show_layout(cr, &name);
    }
}
pub fn draw_strip(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    w: f64,
    detail: &DayDetail,
    colors: &HashMap<String, Rgb>,
    hovered: Option<&MapItem>,
    only: Option<&MapItem>,
) {
    let fg = area.color();
    cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.08);
    rounded(cr, 0., 0., w, 10., 5.);
    let _ = cr.fill();
    let denominator = (detail.end_ms - detail.start_ms).max(1) as f64;
    let mut sessions: Vec<_> = detail
        .devices
        .iter()
        .flat_map(|d| {
            d.items
                .iter()
                .flat_map(move |i| i.sessions.iter().map(move |s| (&d.device.id, i, s)))
        })
        .collect();
    sessions.sort_by_key(|(_, _, s)| s.start_ms);
    for (device, item, session) in sessions {
        if only.is_some_and(|cell| !cell.sessions().contains(session)) {
            continue;
        }
        let hot = hovered.is_none_or(|cell| {
            cell.device_id == *device
                && (cell.item.key == item.key || cell.hidden.iter().any(|i| i.key == item.key))
                && cell.session.as_ref().is_none_or(|s| s == session)
        });
        let color = colors
            .get(&item.key)
            .copied()
            .unwrap_or_else(|| colors::hashed(&item.key).rgb());
        source(cr, color, if hot { 1. } else { 0.18 });
        let x = (session.start_ms - detail.start_ms) as f64 / denominator * w;
        let width = ((session.end_ms - session.start_ms) as f64 / denominator * w).max(2.);
        rounded(cr, x, 0., width, if only.is_some() { 8. } else { 10. }, 2.);
        let _ = cr.fill();
    }
    let start = glib::DateTime::from_unix_local(detail.start_ms / 1000).unwrap();
    for hour in [0, 6, 12, 18, 24] {
        let instant = if hour == 24 {
            detail.end_ms
        } else {
            glib::DateTime::new(
                &start.timezone(),
                start.year(),
                start.month(),
                start.day_of_month(),
                hour,
                0,
                0.,
            )
            .unwrap()
            .to_unix()
                * 1000
        };
        let layout = text_layout(area, &format!("{hour:02}"), 12., false);
        let (tw, _) = layout.pixel_size();
        let x = if hour == 0 {
            0.
        } else if hour == 24 {
            w - tw as f64
        } else {
            (instant - detail.start_ms) as f64 / denominator * w - tw as f64 / 2.
        };
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.65);
        cr.move_to(x, 13.);
        pangocairo::functions::show_layout(cr, &layout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "needs an X server; verify.sh runs it"]
    fn gtk_tiles_focus_drill_hover_and_sample_icons() {
        adw::init().unwrap();
        crate::ui::load_css();
        let date = glib::DateTime::from_local(2026, 10, 28, 0, 0, 0.).unwrap();
        let detail = crate::demo::Demo::new().day(day::query(&date));
        let opened = Rc::new(RefCell::new(Drill::Day));
        let map = Treemap::new(
            detail.clone(),
            Drill::Day,
            HashMap::new(),
            {
                let opened = opened.clone();
                Rc::new(move |drill| *opened.borrow_mut() = drill)
            },
            Rc::new(|_| {}),
        );
        let window = gtk::Window::builder()
            .child(&map)
            .default_width(720)
            .default_height(400)
            .build();
        window.present();
        let settle = || {
            let main = glib::MainLoop::new(None, false);
            glib::timeout_add_local_once(Duration::from_millis(350), {
                let main = main.clone();
                move || main.quit()
            });
            main.run();
        };
        settle();
        for (width, height) in [(648, 300), (1208, 300), (900, 450), (1600, 800)] {
            map.allocate(width, height, -1, None);
            let (groups, tiles) = cells(
                &detail,
                &Drill::Day,
                Rect::new(0., 0., width as f64, height as f64),
            );
            let expected: HashMap<_, _> = groups
                .into_iter()
                .chain(tiles.into_iter().map(|(item, rect)| (item.id, rect)))
                .collect();
            let children = map.imp().children.borrow();
            let mut visible = 0;
            for child in children.iter().filter(|c| c.button.is_child_visible()) {
                visible += 1;
                let rect = expected
                    .get(&child.id)
                    .expect("visible button must have a matching layout item");
                if let Some(item) = &child.item {
                    assert_eq!(child.id, item.borrow().id);
                }
                let bounds = child.button.compute_bounds(&map).unwrap();
                assert_eq!(bounds.x(), rect.x.round() as f32, "{}", child.id);
                assert_eq!(bounds.y(), rect.y.round() as f32, "{}", child.id);
                assert_eq!(
                    bounds.width(),
                    ((rect.x + rect.w).round() - rect.x.round()) as f32,
                    "{}",
                    child.id
                );
                assert_eq!(
                    bounds.height(),
                    ((rect.y + rect.h).round() - rect.y.round()) as f32,
                    "{}",
                    child.id
                );
            }
            assert_eq!(
                visible,
                expected.values().filter(|r| r.w > 0. && r.h > 0.).count()
            );
        }
        let button = map
            .imp()
            .children
            .borrow()
            .iter()
            .find(|c| c.item.is_some() && c.button.is_child_visible())
            .unwrap()
            .button
            .clone();
        assert!(button.width() > 0 && button.height() > 0);
        assert!(gtk::test_accessible_has_property(
            &button,
            gtk::AccessibleProperty::Label
        ));
        assert!(gtk::test_accessible_has_property(
            &button,
            gtk::AccessibleProperty::Description
        ));
        map.focus_first();
        assert!(button.has_focus());
        button.emit_clicked();
        assert_eq!(
            *opened.borrow(),
            Drill::Item("demo-mac".into(), "app:com.apple.dt.Xcode".into())
        );
        let cell = map
            .imp()
            .children
            .borrow()
            .iter()
            .find_map(|c| c.item.as_ref().map(|i| i.borrow().clone()))
            .unwrap();
        map.enter(&button, &cell, false);
        assert!(map.imp().popover.borrow().is_none());
        settle();
        assert!(
            map.imp()
                .popover
                .borrow()
                .as_ref()
                .is_some_and(|p| p.is_visible())
        );
        let popover = map.imp().popover.borrow().clone().unwrap();
        let mut updated = detail.clone();
        updated.devices[0].items[0].total_ms += 60_000;
        map.update(updated, HashMap::new());
        settle();
        assert!(button.has_focus());
        assert_eq!(map.imp().popover.borrow().as_ref(), Some(&popover));
        assert_eq!(map.imp().hovered.borrow().as_ref().unwrap().id, cell.id);
        assert_eq!(
            popover.pointing_to(),
            (
                true,
                gdk::Rectangle::new(0, 0, button.width(), button.height())
            )
        );
        assert!(map.dismiss_hover());
        assert!(!map.dismiss_hover());
        assert!(map.imp().popover.borrow().is_none());
        let mut with_other = detail.clone();
        let mut tiny = with_other.devices[0].items[0].clone();
        tiny.key = "app:tiny".into();
        tiny.name = "Tiny".into();
        tiny.total_ms = 60_000;
        tiny.sessions = vec![spentday_core::Session {
            start_ms: detail.start_ms + 1000,
            end_ms: detail.start_ms + 61_000,
        }];
        with_other.devices[0].items.push(tiny);
        with_other.devices[0].total_ms += 60_000;
        with_other.device_ms += 60_000;
        map.update(with_other, HashMap::new());
        settle();
        let other = map
            .imp()
            .children
            .borrow()
            .iter()
            .find_map(|child| {
                child
                    .item
                    .as_ref()
                    .filter(|item| !item.borrow().hidden.is_empty())
                    .map(|_| child.button.clone())
            })
            .unwrap();
        other.grab_focus();
        other.emit_clicked();
        assert!(
            map.imp()
                .popover
                .borrow()
                .as_ref()
                .is_some_and(|p| p.is_visible() && !p.is_autohide())
        );
        assert!(map.dismiss_hover());
        assert!(map.imp().hovered.borrow().is_none());
        assert!(other.has_focus());
        let mut icons = super::super::icons::IconSampler::new();
        let mut item = detail.devices[0].items[0].clone();
        item.app_key = "chromium".into();
        item.app_ids = vec!["chromium".into()];
        let mut local = detail.clone();
        local.devices[0].items = vec![item.clone()];
        let resolved = super::super::icons::resolve(&local);
        assert_ne!(
            icons.seed(&item, &resolved["chromium"], &window),
            colors::hashed("chromium")
        );
        item.app_key = "remote.app.without.local.match".into();
        item.app_ids = vec![item.app_key.clone()];
        assert_eq!(
            icons.seed(&item, &[], &window),
            colors::hashed(&item.app_key)
        );
        window.close();
    }
}
