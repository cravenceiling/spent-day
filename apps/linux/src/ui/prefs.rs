use crate::app::{self, AppState, SyncStatus};
use crate::format;
use adw::prelude::*;
use gtk::glib;
use spentday_core::{SpentDayError, SyncConfig};
use std::rc::Rc;

pub fn present(parent: &adw::ApplicationWindow, state: &Rc<AppState>) {
    let dialog = adw::PreferencesDialog::builder().title("Preferences").build();
    dialog.connect_closed(glib::clone!(#[weak] state, move |_| {
        state.on_sync_change.take();
    }));
    load(&dialog, state, Some(parent.clone()));
}

/// Reads the sync config off the main thread, then replaces the page. The first load presents
/// the dialog once the page exists, so it opens at its full size.
fn load(dialog: &adw::PreferencesDialog, state: &Rc<AppState>, present_on: Option<adw::ApplicationWindow>) {
    let store = state.store.clone();
    let (dialog, state) = (dialog.clone(), state.clone());
    app::spawn(move || store.sync_config(), move |config| {
        let config = config.unwrap_or_else(|e| {
            dialog.add_toast(adw::Toast::new(&format::error("could not read sync settings", &e)));
            None
        });
        if let Some(old) = dialog.visible_page() {
            dialog.remove(&old);
        }
        dialog.add(&page(&dialog, &state, config));
        if let Some(parent) = present_on {
            dialog.present(Some(&parent));
        }
    });
}

fn page(dialog: &adw::PreferencesDialog, state: &Rc<AppState>, config: Option<SyncConfig>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder().title("Sync").icon_name("emblem-synchronizing-symbolic").build();
    let intro = adw::PreferencesGroup::builder()
        .title("Sync")
        .description("Sync combines your devices into one dashboard. It sends which apps and domains you used, when, and this device's name. All of it is encrypted on this device with your sync code, so the server only stores data it cannot read.")
        .build();
    page.add(&intro);
    match config {
        None => {
            state.on_sync_change.take();
            let server = adw::EntryRow::builder().title("Server URL").input_purpose(gtk::InputPurpose::Url).build();
            intro.add(&server);

            let setup = adw::PreferencesGroup::builder().title("Connect This Device").build();
            let create = gtk::Button::builder().label("Create").valign(gtk::Align::Center).build();
            create.add_css_class("suggested-action");
            let create_row = adw::ActionRow::builder()
                .title("Create a Sync Code")
                .subtitle("Start syncing from this device")
                .activatable_widget(&create)
                .build();
            create_row.add_suffix(&create);
            setup.add(&create_row);
            page.add(&setup);

            // A sync code is 69 characters with its dashes, wider than an entry row, so the
            // field wraps between groups and the whole code stays in view for checking.
            let join = gtk::TextView::builder()
                .wrap_mode(gtk::WrapMode::WordChar)
                .monospace(true)
                .accepts_tab(false)
                .top_margin(12)
                .bottom_margin(12)
                .left_margin(12)
                .right_margin(12)
                .height_request(64)
                .build();
            join.add_css_class("card");
            join.update_property(&[gtk::accessible::Property::Label("Sync code")]);
            let join_button = gtk::Button::builder().label("Join").valign(gtk::Align::Center).build();
            let join_group = adw::PreferencesGroup::builder()
                .title("Join With an Existing Code")
                .description("Paste the sync code from one of your other devices.")
                .header_suffix(&join_button)
                .build();
            join_group.add(&join);
            page.add(&join_group);

            create.connect_clicked(glib::clone!(#[weak] dialog, #[weak] state, #[weak] server, move |_| {
                let Some(url) = server_url(&dialog, &server) else { return };
                let store = state.store.clone();
                connect(&dialog, &state, move || {
                    let sync_code = store.new_sync_code();
                    store.set_sync_config(Some(SyncConfig { server_url: url, sync_code }))
                });
            }));
            join_button.connect_clicked(glib::clone!(#[weak] dialog, #[weak] state, #[weak] server, #[weak] join, move |_| {
                let Some(url) = server_url(&dialog, &server) else { return };
                let buffer = join.buffer();
                let code = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false).trim().to_owned();
                let store = state.store.clone();
                connect(&dialog, &state, move || store.set_sync_config(Some(SyncConfig { server_url: url, sync_code: code })));
            }));
        }
        Some(config) => {
            let server = adw::ActionRow::builder().title("Server").subtitle(&config.server_url).use_markup(false).subtitle_selectable(true).build();
            server.add_css_class("property");
            intro.add(&server);

            let code_group = adw::PreferencesGroup::builder()
                .title("Sync Code")
                .description("Enter this code on your other devices to join them. Anyone with the code can read your activity, so keep it private.")
                .build();
            let code = adw::ActionRow::builder()
                .title(format!("<span font_family=\"monospace\">{}</span>", glib::markup_escape_text(&config.sync_code)))
                .title_selectable(true)
                .build();
            let copy = gtk::Button::builder()
                .icon_name("edit-copy-symbolic")
                .tooltip_text("Copy Sync Code")
                .valign(gtk::Align::Center)
                .build();
            copy.add_css_class("flat");
            copy.connect_clicked(glib::clone!(#[weak] dialog, move |button| {
                button.clipboard().set_text(&config.sync_code);
                dialog.add_toast(adw::Toast::new("Sync code copied"));
            }));
            code.add_suffix(&copy);
            code_group.add(&code);
            page.add(&code_group);

            let status_group = adw::PreferencesGroup::builder().title("Status").build();
            let status = adw::ActionRow::builder().title("Last Sync").use_markup(false).build();
            status.add_css_class("property");
            let sync_button = gtk::Button::builder().label("Sync Now").valign(gtk::Align::Center).build();
            sync_button.connect_clicked(glib::clone!(#[weak] state, move |_| app::sync(&state)));
            status.add_suffix(&sync_button);
            status_group.add(&status);
            page.add(&status_group);
            let update = glib::clone!(#[weak] state, #[weak] status, #[weak] sync_button, move || {
                let sync = state.sync.borrow();
                status.set_subtitle(&status_text(&sync));
                sync_button.set_sensitive(!sync.running);
            });
            update();
            *state.on_sync_change.borrow_mut() = Some(Rc::new(update));

            let off_group = adw::PreferencesGroup::new();
            let off = adw::ButtonRow::builder().title("Turn Off Sync").build();
            off.add_css_class("destructive-action");
            off.connect_activated(glib::clone!(#[weak] dialog, #[weak] state, move |_| confirm_off(&dialog, &state)));
            off_group.add(&off);
            page.add(&off_group);
        }
    }
    page
}

fn server_url(dialog: &adw::PreferencesDialog, row: &adw::EntryRow) -> Option<String> {
    let url = row.text().trim().trim_end_matches('/').to_owned();
    let valid = ["https://", "http://"].iter().any(|scheme| url.len() > scheme.len() && url.starts_with(scheme));
    if !valid {
        row.add_css_class("error");
        row.grab_focus();
        dialog.add_toast(adw::Toast::new("Enter the server address, starting with https://"));
        return None;
    }
    row.remove_css_class("error");
    Some(url)
}

/// Stores the config, then rebuilds the page and runs a first sync.
fn connect(dialog: &adw::PreferencesDialog, state: &Rc<AppState>, work: impl FnOnce() -> Result<(), SpentDayError> + Send + 'static) {
    let (dialog, state) = (dialog.clone(), state.clone());
    app::spawn(work, move |result| match result {
        Ok(()) => {
            load(&dialog, &state, None);
            app::sync(&state);
        }
        Err(SpentDayError::InvalidSyncCode) => {
            dialog.add_toast(adw::Toast::new("That sync code is not valid. Check it for typos."));
        }
        // A bad server address, such as plain http:// to a host outside this network.
        Err(e) => dialog.add_toast(adw::Toast::new(&format::error("could not turn on sync", &e))),
    });
}

fn confirm_off(dialog: &adw::PreferencesDialog, state: &Rc<AppState>) {
    let alert = adw::AlertDialog::builder()
        .heading("Turn Off Sync?")
        .body("This device stops sending and receiving activity. Data already on the server stays there, and you can join again later with your sync code.")
        .close_response("cancel")
        .default_response("cancel")
        .build();
    alert.add_responses(&[("cancel", "Cancel"), ("off", "Turn Off")]);
    alert.set_response_appearance("off", adw::ResponseAppearance::Destructive);
    alert.connect_response(Some("off"), glib::clone!(#[weak] dialog, #[weak] state, move |_, _| {
        let store = state.store.clone();
        let (dialog2, state2) = (dialog.clone(), state.clone());
        app::spawn(move || store.set_sync_config(None), move |result| {
            if let Err(e) = result {
                dialog2.add_toast(adw::Toast::new(&format::error("could not turn off sync", &e)));
            }
            state2.sync.borrow_mut().last = None;
            load(&dialog2, &state2, None);
        });
    }));
    alert.present(Some(dialog));
}

fn status_text(sync: &SyncStatus) -> String {
    if sync.running {
        return "Syncing…".into();
    }
    match &sync.last {
        None => "Not synced since Spent Day started".into(),
        Some(Ok(report)) => {
            let at = glib::DateTime::from_unix_local(report.finished_at_ms / 1000)
                .and_then(|t| t.format("%H:%M"))
                .map(|s| s.to_string())
                .unwrap_or_default();
            let mut text = format!("At {at} · {} sent, {} received", report.pushed_spans, report.pulled_spans);
            match report.skipped_batches {
                0 => {}
                1 => text.push_str(" · 1 batch could not be read"),
                n => text.push_str(&format!(" · {n} batches could not be read")),
            }
            text
        }
        Some(Err(text)) => text.clone(),
    }
}
