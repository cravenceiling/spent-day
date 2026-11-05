use crate::{
    colors::{self, Lch, Rgb},
    desktop::Entries,
};
use adw::prelude::*;
use gtk::{gdk, gio};
use spentday_core::{DayDetail, DayItem, ItemKind};
use std::collections::HashMap;

pub fn resolve(detail: &DayDetail) -> HashMap<String, Vec<String>> {
    let mut entries = Entries::new();
    detail
        .devices
        .iter()
        .flat_map(|d| &d.items)
        .filter(|item| item.kind == ItemKind::App)
        .map(|item| {
            let icons = item
                .app_ids
                .iter()
                .chain(std::iter::once(&item.app_key))
                .filter_map(|id| entries.info(id).icon.clone())
                .collect();
            (item.app_key.clone(), icons)
        })
        .collect()
}

pub struct IconSampler {
    seeds: HashMap<String, Lch>,
}
impl IconSampler {
    pub fn new() -> Self {
        Self {
            seeds: HashMap::new(),
        }
    }
    pub fn seed(
        &mut self,
        item: &DayItem,
        icons: &[String],
        widget: &impl IsA<gtk::Widget>,
    ) -> Lch {
        if let Some(seed) = self.seeds.get(&item.app_key) {
            return *seed;
        }
        let seed = self
            .sample(icons, widget)
            .unwrap_or_else(|| colors::hashed(&item.app_key));
        self.seeds.insert(item.app_key.clone(), seed);
        seed
    }
    fn sample(&self, icons: &[String], widget: &impl IsA<gtk::Widget>) -> Option<Lch> {
        let theme = gtk::IconTheme::for_display(&widget.display());
        let paintable = icons.iter().find_map(|icon| {
            if icon.starts_with('/') {
                Some(theme.lookup_by_gicon(
                    &gio::FileIcon::new(&gio::File::for_path(icon)),
                    32,
                    1,
                    gtk::TextDirection::None,
                    gtk::IconLookupFlags::empty(),
                ))
            } else if theme.has_icon(icon) {
                Some(theme.lookup_icon(
                    icon,
                    &[],
                    32,
                    1,
                    gtk::TextDirection::None,
                    gtk::IconLookupFlags::empty(),
                ))
            } else {
                None
            }
        })?;
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, 32., 32.);
        let node = snapshot.to_node()?;
        let native = widget.native()?;
        let renderer = native.renderer()?;
        let texture =
            renderer.render_texture(&node, Some(&gtk::graphene::Rect::new(0., 0., 32., 32.)));
        let mut downloader = gdk::TextureDownloader::new(&texture);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = downloader.download_bytes();
        let mut pixels = Vec::with_capacity(1024);
        for y in 0..32 {
            for x in 0..32 {
                let i = y * stride + x * 4;
                pixels.push((
                    Rgb {
                        r: bytes[i] as f64 / 255.,
                        g: bytes[i + 1] as f64 / 255.,
                        b: bytes[i + 2] as f64 / 255.,
                    },
                    bytes[i + 3] as f64 / 255.,
                ));
            }
        }
        Some(colors::dominant(pixels).unwrap_or(Lch {
            l: 0.37,
            c: 0.,
            h: 0.,
        }))
    }
}
