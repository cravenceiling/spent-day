//! The process: the store, the three windows, background threads, signals and the exit code.

use crate::demo::Demo;
use crate::platform::{self, Instance, Pref};
use crate::store::Store;
use crate::sync::{self, SyncState, Trigger};
use crate::ui::dash::{Dash, Overlay};
use crate::ui::dash_view;
use crate::ui::fatal::{self, Fatal};
use crate::ui::panel::{self, Panel};
use crate::ui::settings;
use crate::{day, icons, scenes};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use rune::{App, AppHandle, AppSpec, Chord, Key, RunOptions, WindowSpec};
use spentday_core::SpentDay;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    /// The dashboard on Linux; only the menu bar item on macOS (unless onboarding is due).
    Default,
    Background,
    Dashboard,
    Settings,
}

pub struct Opts {
    pub launch: Launch,
    pub day: Option<Date>,
    pub demo: bool,
    /// Arguments for Rune's tooling.
    pub rune: Vec<String>,
}

pub struct Shared {
    pub store: Arc<dyn Store>,
    pub demo: bool,
    pub tz: TimeZone,
    pub icons: icons::Seeds,
    /// The one source of truth for sync status.
    pub sync: Mutex<SyncState>,
    /// Set once the windows exist, before the loop starts.
    pub ui: OnceLock<Handles>,
}

pub struct Handles {
    pub dash: AppHandle<Dash>,
    pub panel: AppHandle<Panel>,
    pub fatal: AppHandle<Fatal>,
}

/// Shared by the panel's focus hook (a plain `fn`) and the status item's click.
#[cfg(target_os = "macos")]
static PANEL: std::sync::LazyLock<Mutex<platform::status_item::PanelState>> =
    std::sync::LazyLock::new(Mutex::default);

/// Exit code 3 after the fatal window showed: `spentday.service` doesn't restart on it.
static FAILED: AtomicBool = AtomicBool::new(false);

pub fn show_fatal(shared: &Shared, fatal: Fatal) {
    FAILED.store(true, Ordering::Release);
    if let Some(ui) = shared.ui.get() {
        ui.dash.close();
        ui.panel.close();
        ui.fatal.update(move |s, _| *s = fatal);
        ui.fatal.open();
    }
}

fn open_core() -> Result<Arc<SpentDay>, String> {
    let path = platform::db_path()?;
    SpentDay::open(path, platform::device_name(), platform::PLATFORM)
        .map_err(|e| crate::format::error("open the database", &e))
}

pub fn run(opts: Opts) -> ExitCode {
    let headless = rune::tooling::Args::parse_from(opts.rune.clone()).headless();
    // Headless renders never open the user's database.
    let demo = opts.demo || headless;
    rune::set_app_id(if demo {
        "day.spent.SpentDay.Demo"
    } else {
        "day.spent.SpentDay"
    });
    let (store, core, startup_error): (Arc<dyn Store>, _, _) = if demo {
        (Arc::new(Demo::new()), None, None)
    } else {
        match open_core() {
            Ok(core) => (core.clone(), Some(core), None),
            // The windows still need a store; the dashboard never opens.
            Err(e) => (Arc::new(Demo::new()), None, Some(e)),
        }
    };
    let shared = Arc::new(Shared {
        store,
        demo,
        tz: TimeZone::system(),
        icons: icons::Seeds::default(),
        sync: Mutex::default(),
        ui: OnceLock::new(),
    });
    let selected = opts.day.unwrap_or_else(|| {
        if demo {
            jiff::civil::date(2026, 10, 28)
        } else {
            day::today(&shared.tz)
        }
    });

    let dash_app = {
        let shared = shared.clone();
        let spec = AppSpec {
            scenes: scenes::DASH,
            on_key: Some(Dash::on_key),
            min_size: Some((720.0, 560.0)),
            ..AppSpec::new(
                "Spent Day",
                (1280.0, 800.0),
                || unreachable!(),
                dash_view::view,
            )
        };
        App::new(spec)
            .args(opts.rune.clone())
            .init(move || Dash::new(shared, selected))
            .windowless()
            .release_gpu_when_closed()
            .tab_visits_controls(true)
            .on_focus(|d, focused, tx| {
                if focused {
                    d.refresh(tx);
                }
            })
            .on_reopen(|d, _, tx| {
                if !FAILED.load(Ordering::Acquire) {
                    d.refresh(tx);
                }
                if let Some(ui) = d.shared.ui.get() {
                    if FAILED.load(Ordering::Acquire) {
                        ui.fatal.open();
                    } else {
                        ui.dash.open();
                    }
                }
                true
            })
    };
    let panel_app = {
        let shared = shared.clone();
        let spec = AppSpec {
            scenes: scenes::PANEL,
            on_key: Some(Panel::on_key),
            ..AppSpec::new("Spent Day", panel::SIZE, || unreachable!(), panel::view)
        };
        App::new(spec)
            .args(opts.rune.clone())
            .init(move || Panel::new(shared))
            .windowless()
            .on_focus(|p, focused, tx| {
                #[cfg(target_os = "macos")]
                if PANEL.lock().unwrap().focus_changed(focused) {
                    tx.close_window();
                }
                if focused {
                    p.refresh(tx);
                }
            })
    };
    let fatal_app = App::new(AppSpec {
        scenes: scenes::FATAL,
        on_key: Some(|_, ev, tx| {
            if ev.key == Key::Escape || Chord::parse("mod+q").unwrap().matches_tx(ev, tx) {
                tx.quit();
                true
            } else {
                false
            }
        }),
        ..AppSpec::new("Spent Day", fatal::SIZE, Fatal::default, fatal::view)
    })
    .args(opts.rune.clone())
    .windowless()
    .on_close(|_, tx| tx.quit());
    let handles = Handles {
        dash: dash_app.handle(),
        panel: panel_app.handle(),
        fatal: fatal_app.handle(),
    };
    #[cfg(target_os = "macos")]
    let panel = handles.panel.clone();
    let dash = handles.dash.clone();
    let _ = shared.ui.set(handles);

    let mut tracking = None;
    let mut ticking = None;
    let mut _instance = None;
    if !headless {
        let onboarding = platform::read_pref(Pref::OnboardingDone);
        let show = match opts.launch {
            Launch::Background => false,
            Launch::Dashboard | Launch::Settings => true,
            Launch::Default => cfg!(target_os = "linux") || onboarding == Some(false),
        };
        let activate = dash.clone();
        let fatal = shared.ui.get().unwrap().fatal.clone();
        let on_activate = Box::new(move |show: bool, token: String| {
            if show {
                if FAILED.load(Ordering::Acquire) {
                    fatal.activate(token);
                } else {
                    activate.activate(token);
                }
            }
        });
        match platform::claim_instance(demo, show, on_activate) {
            #[cfg(target_os = "linux")]
            Instance::Forwarded => return ExitCode::SUCCESS,
            Instance::Primary(guard) => _instance = Some(guard),
        }
        quit_on_signals(dash.clone());
        {
            let dash = dash.clone();
            // Runs on the main thread once the loop starts, with or without a window.
            dash.clone().update(move |_, _| {
                #[cfg(target_os = "macos")]
                platform::menu_bar(Box::new(move |rect| {
                    PANEL.lock().unwrap().toggle(&panel, rect, panel::SIZE);
                }));
                platform::install_quit_handler(Box::new(move || dash.quit()));
            });
        }
        if let Some(e) = startup_error {
            show_fatal(
                &shared,
                Fatal {
                    title: "Cannot open the database".into(),
                    body: e,
                },
            );
        } else {
            if let Some(core) = core {
                if let Err(e) = core.start_web_listener(7853) {
                    eprintln!("spentday: browser extension listener on port 7853: {e}");
                }
                let on_fatal_shared = shared.clone();
                let on_fatal = Box::new(move |body: String| {
                    show_fatal(
                        &on_fatal_shared,
                        Fatal {
                            title: "Spent Day can't track apps".into(),
                            body,
                        },
                    )
                });
                tracking = Some(platform::start_tracking(core, on_fatal));
            }
            if !demo {
                ticking = Some(ticker(shared.clone()));
            }
            dash.update(move |d, tx| {
                d.refresh(tx);
                if opts.launch == Launch::Settings {
                    settings::open(d, tx);
                } else {
                    // Seeds `configured`, so Sync Now is offered before the first sync ends.
                    settings::load(d, tx);
                    if onboarding == Some(false) {
                        d.overlay = Overlay::Onboarding;
                    }
                }
            });
            if show {
                dash.open();
            }
        }
    }

    #[cfg(target_os = "macos")]
    let panel_options = platform::status_item::panel_options(panel::SIZE);
    // Linux has no status item; the panel exists for headless scenes only.
    #[cfg(target_os = "linux")]
    let panel_options = rune::WindowOptions {
        visible: false,
        ..Default::default()
    }
    .into();
    let result = rune::try_run_windows_with_args(
        RunOptions {
            accessory: cfg!(target_os = "macos"),
        },
        opts.rune,
        move |_| {
            vec![
                WindowSpec::from_app("dash", dash_app).boxed(),
                WindowSpec::from_app("panel", panel_app)
                    .configured_options(panel_options)
                    .boxed(),
                WindowSpec::from_app("fatal", fatal_app).boxed(),
            ]
        },
    );
    drop(ticking);
    if let Some(tracking) = tracking {
        tracking.stop();
    }
    match result {
        Err(e) => {
            eprintln!("spentday: {e}");
            ExitCode::FAILURE
        }
        Ok(()) if FAILED.load(Ordering::Acquire) => ExitCode::from(3),
        Ok(()) => ExitCode::SUCCESS,
    }
}

/// SIGTERM and SIGINT take the same quit path as the Quit command, so tracking flushes after
/// the loop returns.
fn quit_on_signals(dash: AppHandle<Dash>) {
    use signal_hook::consts::{SIGINT, SIGTERM};
    match signal_hook::iterator::Signals::new([SIGTERM, SIGINT]) {
        Ok(mut signals) => {
            std::thread::spawn(move || {
                if signals.forever().next().is_some() {
                    dash.quit();
                }
            });
        }
        Err(e) => eprintln!("spentday: can't watch for SIGTERM: {e}"),
    }
}

/// Refreshes the dashboard every 30 s; syncs 10 s after start, then every 300 s.
fn ticker(shared: Arc<Shared>) -> Sender<()> {
    let (stop, stopped) = mpsc::channel();
    std::thread::spawn(move || {
        let mut tick = 0u64;
        while stopped.recv_timeout(Duration::from_secs(10)) == Err(mpsc::RecvTimeoutError::Timeout)
        {
            tick += 1;
            if let Some(ui) = shared.ui.get()
                && tick.is_multiple_of(3)
            {
                // A closed window reloads when it is shown or focused again.
                if ui.dash.is_visible() {
                    ui.dash.update_if(|d, tx| d.refresh(tx));
                }
                if cfg!(target_os = "macos") && ui.panel.is_visible() {
                    ui.panel.update_if(Panel::refresh);
                }
            }
            if !shared.demo && tick % 30 == 1 {
                sync::run(&shared, Trigger::Auto);
            }
        }
    });
    stop
}
