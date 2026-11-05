pub mod dashboard;
mod icons;
pub mod prefs;
mod treemap;

use crate::app::APP_ID;
use adw::prelude::*;
use gtk::gdk;

const CSS: &str = "
.calm-day { background: #f7f7f9; }
.calm-day.dark { background: #161618; }
.spentday-total { font-size: 40px; font-weight: 600; letter-spacing: -1px; font-feature-settings: 'tnum'; }
.map-tile, .map-tile:hover, .map-tile:active { min-width: 0; min-height: 0; padding: 0; margin: 0; border: none; border-radius: 10px; background: none; box-shadow: none; }
.map-tile:focus-visible { outline: none; }
.map-group, .map-group:hover, .map-group:active, .crumb { min-width: 0; min-height: 0; padding: 0; margin: 0; background: none; border: none; box-shadow: none; font-size: 13px; font-weight: 400; }
";

pub fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn about(parent: Option<&gtk::Window>) {
    let dialog = adw::AboutDialog::builder()
        .application_name("Spent Day")
        .application_icon(APP_ID)
        .developer_name("The Spent Day contributors")
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Screen time across all your devices, synced end-to-end encrypted.")
        .license_type(gtk::License::MitX11)
        .build();
    dialog.present(parent);
}
