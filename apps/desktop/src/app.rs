//! The process: the store, the three windows, background threads, signals and the exit code.

use crate::demo::Demo;
use crate::platform::{self, Instance, Pref};
use crate::store::Store;
use crate::sync::{self, SyncState, Trigger};
use crate::ui::dash::{Dash, Overlay};
use crate::ui::dash_view;
use crate::ui::fatal::{self, Fatal};
use crate::ui::panel::{self, Panel};
use crate::{day, icons, scenes};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use rune::{App, AppHandle, AppSpec, MacWindowOptions, RunOptions, WindowOptions, WindowSpec};
use spentday_core::SpentDay;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Exit code 3 after the fatal window showed: `spentday.service` doesn't restart on it.
static FAILED: AtomicBool = AtomicBool::new(false);

pub fn show_fatal(shared: &Shared, fatal: Fatal) {
    FAILED.store(true, Ordering::Release);
    if let Some(ui) = shared.ui.get() {
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
    let selected = opts.day.unwrap_or_else(|| day::today(&shared.tz));

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
            .init(move || Dash::new(shared, selected))
            .windowless()
            .release_gpu_when_closed()
    };
    let panel_app = {
        let shared = shared.clone();
        let spec = AppSpec {
            scenes: scenes::PANEL,
            on_key: Some(Panel::on_key),
            ..AppSpec::new("Spent Day", panel::SIZE, || unreachable!(), panel::view)
        };
        App::new(spec)
            .init(move || Panel::new(shared))
            .windowless()
            .on_focus(Panel::on_focus)
    };
    let fatal_app = App::new(AppSpec {
        scenes: scenes::FATAL,
        ..AppSpec::new("Spent Day", fatal::SIZE, Fatal::default, fatal::view)
    })
    .windowless();
    let handles = Handles {
        dash: dash_app.handle(),
        panel: panel_app.handle(),
        fatal: fatal_app.handle(),
    };
    let (dash, panel) = (handles.dash.clone(), handles.panel.clone());
    let _ = shared.ui.set(handles);

    let mut tracking = None;
    let mut _instance = None;
    if !headless {
        let show = match opts.launch {
            Launch::Background => false,
            Launch::Dashboard | Launch::Settings => true,
            Launch::Default => {
                cfg!(target_os = "linux")
                    || platform::read_pref(Pref::OnboardingDone) == Some(false)
            }
        };
        let activate = dash.clone();
        let on_activate = Box::new(move |show: bool, token: String| {
            if show {
                activate.activate(token)
            }
        });
        match platform::claim_instance(demo, show, on_activate) {
            Instance::Forwarded => return ExitCode::SUCCESS,
            Instance::Primary(guard) => _instance = Some(guard),
        }
        quit_on_signals(dash.clone());
        {
            let (dash, panel) = (dash.clone(), panel.clone());
            // Runs on the main thread once the loop starts, with or without a window.
            dash.clone().update(move |_, _| {
                platform::menu_bar(Box::new(move |_rect| panel.open()));
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
            ticker(shared.clone());
            if show {
                dash.open();
            }
            if opts.launch == Launch::Settings {
                dash.update(|d, _| d.overlay = Overlay::Settings);
            }
        }
    }

    let panel_options = WindowOptions {
        borderless: true,
        transparent: true,
        resizable: false,
        always_on_top: true,
        visible: false,
        min_size: Some(panel::SIZE),
        max_size: Some(panel::SIZE),
        ..Default::default()
    }
    .macos(MacWindowOptions::default().corner_radius(12.0));
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
fn ticker(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut tick = 0u64;
        loop {
            std::thread::sleep(Duration::from_secs(10));
            tick += 1;
            if let Some(ui) = shared.ui.get()
                && tick.is_multiple_of(3)
            {
                ui.dash.update_if(|d, tx| d.refresh(tx));
            }
            if !shared.demo && tick % 30 == 1 {
                sync::run(&shared, Trigger::Auto);
            }
        }
    });
}
