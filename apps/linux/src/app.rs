use crate::demo::Demo;
use crate::store::Store;
use crate::tracker::Tracker;
use crate::ui::{dashboard::Dashboard, prefs};
use crate::{format, paths, ui};
use adw::prelude::*;
use gtk::{gio, glib};
use spentday_core::{SpentDay, SpentDayError, Platform, SyncReport};
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const APP_ID: &str = "day.spent.SpentDay";
const WEB_PORT: u16 = 7853;
const AUTO_SYNC_SECS: u32 = 300;

#[derive(Default)]
pub struct SyncStatus {
    pub running: bool,
    pub last: Option<Result<SyncReport, String>>,
}

pub struct AppState {
    pub store: Arc<dyn Store>,
    pub demo: bool,
    pub sync: RefCell<SyncStatus>,
    /// Set while the preferences dialog is open, so it can redraw its status row.
    pub on_sync_change: RefCell<Option<Rc<dyn Fn()>>>,
    pub dashboard: RefCell<Option<Rc<Dashboard>>>,
    tracker: RefCell<Option<Tracker>>,
    _hold: Option<gio::ApplicationHoldGuard>,
}

pub fn run() -> glib::ExitCode {
    let demo = std::env::args().any(|a| a == "--demo");
    let app = adw::Application::builder()
        .application_id(if demo { "day.spent.SpentDay.Demo" } else { APP_ID })
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.add_main_option("background", glib::Char::from(b'b'), glib::OptionFlags::NONE, glib::OptionArg::None, "Track without opening a window", None);
    app.add_main_option("demo", glib::Char::from(0), glib::OptionFlags::NONE, glib::OptionArg::None, "Show sample data instead of the database", None);

    let state: Rc<OnceCell<Rc<AppState>>> = Rc::default();
    app.connect_startup({
        let state = state.clone();
        move |app| {
            let _ = state.set(startup(app, demo));
        }
    });
    app.connect_command_line(|app, cmd| {
        if !cmd.options_dict().contains("background") {
            app.activate();
        }
        glib::ExitCode::SUCCESS
    });
    app.connect_activate({
        let state = state.clone();
        // A second launch while the startup error dialog is open lands here before startup ends.
        move |app| {
            if let Some(state) = state.get() {
                present(app, state);
            }
        }
    });
    app.connect_shutdown(move |_| {
        if let Some(tracker) = state.get().and_then(|s| s.tracker.take()) {
            tracker.stop();
        }
    });
    let status = app.run();
    if CANNOT_TRACK.load(Ordering::Relaxed) { glib::ExitCode::from(CANNOT_START) } else { status }
}

fn startup(app: &adw::Application, demo: bool) -> Rc<AppState> {
    ui::load_css();
    gtk::Window::set_default_icon_name(APP_ID);

    let (store, tracker, started, hold): (Arc<dyn Store>, _, _, _) = if demo {
        (Arc::new(Demo::new()), None, None, None)
    } else {
        let path = paths::db_path();
        let name = glib::host_name().to_string();
        let opened = paths::utf8(&path).and_then(|utf8| SpentDay::open(utf8, name, Platform::Linux));
        let spentday = opened.unwrap_or_else(|e| {
            eprintln!("spentday: cannot open {}: {e}", path.display());
            cannot_open(&path, &e);
            std::process::exit(CANNOT_START.into());
        });
        if let Err(e) = spentday.start_web_listener(WEB_PORT) {
            eprintln!("spentday: browser extension listener on port {WEB_PORT}: {e}");
        }
        // Closing the window keeps tracking. Only Quit ends the process.
        let (tracker, started) = Tracker::spawn(spentday.clone());
        (spentday, Some(tracker), Some(started), Some(app.hold()))
    };
    let state = Rc::new(AppState {
        store,
        demo,
        sync: RefCell::default(),
        on_sync_change: RefCell::default(),
        dashboard: RefCell::default(),
        tracker: RefCell::new(tracker),
        _hold: hold,
    });

    if let Some(started) = started {
        spawn(move || started.recv().unwrap_or(Ok(())), glib::clone!(#[weak] app, move |result: Result<(), String>| {
            if let Err(e) = result {
                cannot_track(&app, &e);
            }
        }));
    }

    let action = |name: &str, accels: &[&str], f: Box<dyn Fn()>| {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| f());
        app.add_action(&action);
        app.set_accels_for_action(&format!("app.{name}"), accels);
    };
    action("quit", &["<Control>q"], Box::new(glib::clone!(#[weak] app, move || app.quit())));
    action("preferences", &["<Control>comma"], Box::new(glib::clone!(#[weak] app, #[strong] state, move || {
        present(&app, &state);
        if let Some(dashboard) = state.dashboard.borrow().as_ref() {
            prefs::present(&dashboard.window, &state);
        }
    })));
    action("about", &[], Box::new(glib::clone!(#[weak] app, move || ui::about(app.active_window().as_ref()))));
    action("sync", &[], Box::new(glib::clone!(#[strong] state, move || sync(&state))));
    app.set_accels_for_action("window.close", &["<Control>w"]);

    for signal in [2, 15] {
        glib::unix_signal_add_local(signal, glib::clone!(#[weak] app, #[upgrade_or] glib::ControlFlow::Break, move || {
            app.quit();
            glib::ControlFlow::Break
        }));
    }
    if !demo {
        auto_sync(&state);
        glib::timeout_add_seconds_local(AUTO_SYNC_SECS, glib::clone!(#[strong] state, move || {
            auto_sync(&state);
            glib::ControlFlow::Continue
        }));
    }
    state
}

fn present(app: &adw::Application, state: &Rc<AppState>) {
    if let Some(dashboard) = state.dashboard.borrow().as_ref() {
        dashboard.window.present();
        return;
    }
    let dashboard = Dashboard::new(app, state.clone());
    dashboard.window.connect_destroy(glib::clone!(#[weak] state, move |_| {
        state.dashboard.take();
    }));
    dashboard.window.present();
    *state.dashboard.borrow_mut() = Some(dashboard);
}

/// Shows why the database did not open, and returns once the user closes the dialog. Launched
/// from the desktop or at login, Spent Day has no terminal to print to.
fn cannot_open(path: &std::path::Path, e: &SpentDayError) {
    let alert = adw::AlertDialog::builder()
        .heading("Cannot Open the Database")
        .body(match e {
            // Its message already names the file and says what to do.
            SpentDayError::OldDatabase { .. } => e.to_string(),
            _ => format!("{}\n\n{e}", path.display()),
        })
        .close_response("quit")
        .default_response("quit")
        .build();
    alert.add_response("quit", "Quit");
    let done = glib::MainLoop::new(None, false);
    alert.connect_closed(glib::clone!(#[strong] done, move |_| done.quit()));
    alert.present(None::<&gtk::Widget>);
    done.run();
}

/// Set when Spent Day quits because it cannot track, so it exits with `CANNOT_START`.
static CANNOT_TRACK: AtomicBool = AtomicBool::new(false);

/// The exit status after the user closed a "Cannot Open the Database" or "Cannot Track Apps"
/// dialog. Starting again would only show the dialog again, so spentday.service does not
/// restart on it. GTK and GApplication exit 1 for failures that may pass, such as a display
/// that is not up yet, so this is a status of its own.
pub const CANNOT_START: u8 = 3;

/// Tells the user that the focus collector did not start, then quits once they close the
/// dialog. Running on without it would record nothing.
fn cannot_track(app: &adw::Application, e: &str) {
    let alert = adw::AlertDialog::builder()
        .heading("Cannot Track Apps")
        .body(format!("{e}\n\nSpent Day records nothing without this, so it quits now."))
        .close_response("quit")
        .default_response("quit")
        .build();
    alert.add_response("quit", "Quit");
    alert.connect_closed(glib::clone!(#[weak] app, move |_| {
        CANNOT_TRACK.store(true, Ordering::Relaxed);
        app.quit();
    }));
    alert.present(app.active_window().as_ref());
}

/// Runs work off the main thread and hands the result back to it. A panic in `work` is logged.
pub fn spawn<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
    glib::spawn_future_local(async move {
        match gio::spawn_blocking(work).await {
            Ok(value) => done(value),
            Err(_) => eprintln!("spentday: background task panicked"),
        }
    });
}

pub fn sync(state: &Rc<AppState>) {
    run_sync(state, |store| Some(store.sync_now()));
}

/// Like `sync`, but quietly does nothing until sync is configured.
fn auto_sync(state: &Rc<AppState>) {
    run_sync(state, |store| match store.sync_config() {
        Ok(Some(_)) => Some(store.sync_now()),
        Ok(None) => None,
        Err(e) => Some(Err(e)),
    });
}

fn run_sync(state: &Rc<AppState>, work: fn(&dyn Store) -> Option<Result<SyncReport, SpentDayError>>) {
    if state.sync.borrow().running {
        return;
    }
    state.sync.borrow_mut().running = true;
    notify_sync(state);
    let store = state.store.clone();
    let state = state.clone();
    spawn(move || work(store.as_ref()), move |result| {
        let mut sync = state.sync.borrow_mut();
        sync.running = false;
        if let Some(result) = result {
            sync.last = Some(result.map_err(|e| format::error("sync failed", &e)));
        }
        drop(sync);
        notify_sync(&state);
        if let Some(dashboard) = state.dashboard.borrow().as_ref() {
            dashboard.refresh();
        }
    });
}

fn notify_sync(state: &AppState) {
    let callback = state.on_sync_change.borrow().clone();
    if let Some(callback) = callback {
        callback();
    }
}
