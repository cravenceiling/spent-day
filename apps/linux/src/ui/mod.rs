pub mod chart;
pub mod dashboard;
pub mod prefs;

use crate::app::APP_ID;
use adw::prelude::*;
use gtk::gdk;

const CSS: &str = "
.spentday-total { font-size: 44px; font-weight: 800; letter-spacing: -1px; font-feature-settings: 'tnum'; }
.spentday-bar trough, .spentday-bar progress { min-height: 6px; border-radius: 3px; }
.spentday-chart { padding: 18px 18px 8px 18px; }
";

pub fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
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
