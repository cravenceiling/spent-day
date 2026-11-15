use super::{desktop, paths};
use crate::sync::SyncView;
use crate::ui::panel::TodaySummary;
use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};
use std::path::{Path, PathBuf};

const ICON: &str = include_str!("../../../linux/data/icons/spentday-symbolic.svg");

pub struct TrayCallbacks {
    pub open: Box<dyn Fn(String) + Send + Sync>,
    pub settings: Box<dyn Fn(String) + Send + Sync>,
    /// True after a startup failure, when the store is a stand-in and only the fatal window opens.
    pub failed: Box<dyn Fn() -> bool + Send + Sync>,
    pub today: Box<dyn Fn() -> Result<TodaySummary, String> + Send + Sync>,
    pub sync_state: Box<dyn Fn() -> SyncView + Send + Sync>,
    pub sync: Box<dyn Fn() + Send + Sync>,
    pub quit: Box<dyn Fn() + Send + Sync>,
}

struct Tray {
    demo: bool,
    icons: String,
    callbacks: TrayCallbacks,
    total_ms: Option<i64>,
    apps: Vec<AppRow>,
}

/// Resolved in `refresh`, because ksni rebuilds the menu after every click and scroll too.
struct AppRow {
    label: String,
    icon_name: String,
    icon_data: Vec<u8>,
}

/// The menu always holds this many app slots and hides the unused ones. A changing item count
/// renumbers every item while the host opens the menu, and a click on a stale id is dropped.
const APP_SLOTS: usize = 3;
/// dbusmenu sends icon data on every layout change; a larger PNG is not worth the bus traffic.
const MAX_ICON_BYTES: u64 = 256 * 1024;

/// The item lives on ksni's own thread until the process exits, which also releases its bus
/// name. An explicit shutdown would wait on the tray host and could hold up the tracking flush.
pub fn tray(demo: bool, callbacks: TrayCallbacks) {
    let start = || -> Result<(), String> {
        let icons = match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(dir) if !dir.is_empty() && Path::new(&dir).is_absolute() => {
                PathBuf::from(dir).join("spentday/icons")
            }
            _ => PathBuf::from(paths::db_path()?).with_file_name("icons"),
        };
        write_icon(&icons).map_err(|e| format!("write the tray icon: {e}"))?;
        let icons = paths::utf8(&icons).map_err(|e| e.to_string())?;
        let mut tray = Tray {
            demo,
            icons,
            callbacks,
            total_ms: None,
            apps: Vec::new(),
        };
        tray.refresh();
        // Autostart can precede Waybar; ksni keeps exporting and registers when it appears.
        tray.assume_sni_available(true)
            .spawn()
            .map(drop)
            .map_err(|e| e.to_string())
    };
    if let Err(e) = start() {
        eprintln!("spentday: can't start the system tray: {e}");
    }
}

// GTK-based hosts (Waybar before 0.16, GNOME's AppIndicator) search IconThemePath as a theme
// only when it holds an index.theme, so they find just the top-level file. KDE and newer Waybar
// look in hicolor/ first.
fn write_icon(dir: &Path) -> std::io::Result<()> {
    let apps = dir.join("hicolor/scalable/apps");
    std::fs::create_dir_all(&apps)?;
    std::fs::write(dir.join("spentday-symbolic.svg"), ICON)?;
    std::fs::write(apps.join("spentday-symbolic.svg"), ICON)
}

fn activation_token() -> String {
    ksni::current_xdg_activation_token()
        .or_else(|| {
            // KDE can deliver its token just after the click, as ksni's example documents.
            std::thread::sleep(std::time::Duration::from_millis(1));
            ksni::current_xdg_activation_token()
        })
        .unwrap_or_default()
}

fn open(tray: &mut Tray) {
    (tray.callbacks.open)(activation_token());
}

impl ksni::Tray for Tray {
    // Hosts then show the menu on a left click, like the macOS panel, instead of calling Activate.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        if self.demo {
            "day.spent.SpentDay.Demo"
        } else {
            "day.spent.SpentDay"
        }
        .into()
    }

    fn title(&self) -> String {
        "Spent Day".into()
    }

    fn icon_name(&self) -> String {
        "spentday-symbolic".into()
    }

    fn icon_theme_path(&self) -> String {
        self.icons.clone()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let failed = (self.callbacks.failed)();
        let sync = (self.callbacks.sync_state)();
        // GTK greys out disabled items, so the summary rows stay enabled and open the dashboard.
        let mut menu = vec![
            StandardItem {
                label: match self.total_ms {
                    Some(ms) => format!("Today  {}", crate::format::duration(ms)),
                    None => "Today".into(),
                },
                activate: Box::new(open),
                ..Default::default()
            }
            .into(),
        ];
        menu.extend((0..APP_SLOTS).map(|i| {
            let row = self.apps.get(i);
            StandardItem {
                label: row.map(|r| r.label.clone()).unwrap_or_default(),
                icon_name: row.map(|r| r.icon_name.clone()).unwrap_or_default(),
                icon_data: row.map(|r| r.icon_data.clone()).unwrap_or_default(),
                visible: row.is_some(),
                activate: Box::new(open),
                ..Default::default()
            }
            .into()
        }));
        menu.extend([
            MenuItem::Separator,
            StandardItem {
                label: "Open Dashboard".into(),
                activate: Box::new(open),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if sync.running {
                    "Syncing…"
                } else {
                    "Sync Now"
                }
                .into(),
                enabled: sync.configured && !sync.running && !failed,
                activate: Box::new(|tray: &mut Self| (tray.callbacks.sync)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings…".into(),
                enabled: !failed,
                activate: Box::new(|tray: &mut Self| {
                    (tray.callbacks.settings)(activation_token());
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit Spent Day".into(),
                activate: Box::new(|tray: &mut Self| (tray.callbacks.quit)()),
                ..Default::default()
            }
            .into(),
        ]);
        // dbusmenu interprets a single underscore as a mnemonic, including in app names.
        for item in &mut menu {
            if let MenuItem::Standard(item) = item {
                item.label = item.label.replace('_', "__");
            }
        }
        menu
    }

    // ksni rebuilds the host's cached menu after this hook, including sync and startup state.
    fn menu_about_to_show(&mut self) {
        self.refresh();
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        eprintln!("spentday: no system tray host; tracking continues");
        true
    }
}

impl Tray {
    fn refresh(&mut self) {
        // After a failure, or past midnight, the last totals would pass for today's.
        self.total_ms = None;
        self.apps.clear();
        if (self.callbacks.failed)() {
            return;
        }
        let today = match (self.callbacks.today)() {
            Ok(today) => today,
            Err(e) => return eprintln!("spentday: can't refresh the tray: {e}"),
        };
        let mut entries = desktop::Entries::new();
        self.total_ms = Some(today.detail.wall_ms);
        self.apps = today
            .apps
            .iter()
            .take(APP_SLOTS)
            .map(|app| {
                let mut row = AppRow {
                    label: format!(
                        "{}  {}",
                        app.app_name,
                        crate::format::duration(app.total_ms)
                    ),
                    icon_name: String::new(),
                    icon_data: Vec::new(),
                };
                let icon = app
                    .app_ids
                    .iter()
                    .chain(std::iter::once(&app.app_key))
                    .find_map(|id| entries.info(id).icon.clone());
                if let Some(icon) = icon {
                    let path = Path::new(&icon);
                    if !path.is_absolute() {
                        if !icon.contains('/') {
                            row.icon_name = icon;
                        }
                    } else if path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
                        && std::fs::metadata(path).is_ok_and(|m| m.len() <= MAX_ICON_BYTES)
                    {
                        row.icon_data = std::fs::read(path).unwrap_or_default();
                    }
                }
                row
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;

    fn demo_today() -> Result<TodaySummary, String> {
        let tz = jiff::tz::TimeZone::UTC;
        let demo = crate::demo::Demo::with_timezone(tz.clone());
        let q = crate::day::query(jiff::civil::date(2026, 10, 28), &tz);
        let summary = demo.summary(spentday_core::SummaryQuery {
            start_ms: q.start_ms,
            end_ms: q.end_ms,
            bucket_ms: 3_600_000,
            device_id: None,
        });
        Ok(TodaySummary {
            detail: demo.day(q),
            apps: summary.apps.into_iter().take(3).collect(),
        })
    }

    fn tray() -> Tray {
        Tray {
            demo: false,
            icons: "/tmp/icons".into(),
            total_ms: None,
            apps: Vec::new(),
            callbacks: TrayCallbacks {
                open: Box::new(|_| {}),
                settings: Box::new(|_| {}),
                failed: Box::new(|| false),
                today: Box::new(demo_today),
                sync_state: Box::new(SyncView::default),
                sync: Box::new(|| {}),
                quit: Box::new(|| {}),
            },
        }
    }

    fn visible_labels(tray: &Tray) -> Vec<String> {
        tray.menu()
            .iter()
            .filter_map(|item| match item {
                MenuItem::Standard(item) if item.visible => Some(item.label.clone()),
                MenuItem::Standard(_) => None,
                MenuItem::Separator => Some("---".into()),
                _ => panic!("unexpected menu item"),
            })
            .collect()
    }

    fn item<'a>(menu: &'a [MenuItem<Tray>], label: &str) -> &'a StandardItem<Tray> {
        menu.iter()
            .find_map(|item| match item {
                MenuItem::Standard(item) if item.label == label => Some(item),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {label}"))
    }

    #[test]
    fn today_menu_labels_and_order() {
        let mut tray = tray();
        tray.refresh();
        assert_eq!(
            visible_labels(&tray),
            [
                "Today  13h 50m",
                "Xcode  6h 5m",
                "Instagram  1h 35m",
                "YouTube  1h 20m",
                "---",
                "Open Dashboard",
                "Sync Now",
                "Settings…",
                "---",
                "Quit Spent Day",
            ]
        );
        let opens = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let clicked = opens.clone();
        tray.callbacks.open = Box::new(move |_| {
            clicked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        for item in tray.menu().into_iter().take(1 + APP_SLOTS) {
            let MenuItem::Standard(item) = item else {
                panic!("missing summary row")
            };
            assert!(item.enabled);
            (item.activate)(&mut tray);
        }
        assert_eq!(opens.load(std::sync::atomic::Ordering::SeqCst), 4);
    }

    #[test]
    fn item_count_stays_fixed_when_the_day_has_fewer_apps() {
        let mut tray = tray();
        tray.refresh();
        let full = tray.menu().len();
        tray.callbacks.today = Box::new(|| {
            let mut today = demo_today()?;
            today.detail.wall_ms = 0;
            today.apps.clear();
            Ok(today)
        });
        tray.menu_about_to_show();
        assert_eq!(tray.menu().len(), full);
        assert_eq!(visible_labels(&tray)[..2], ["Today  0m", "---"]);
    }

    #[test]
    fn a_failed_load_or_startup_drops_the_last_totals() {
        let mut stale = tray();
        stale.refresh();
        stale.callbacks.today = Box::new(|| Err("locked".into()));
        stale.menu_about_to_show();
        assert_eq!(visible_labels(&stale)[..2], ["Today", "---"]);

        let mut failed = tray();
        failed.callbacks.failed = Box::new(|| true);
        failed.callbacks.sync_state = Box::new(|| SyncView {
            configured: true,
            ..Default::default()
        });
        failed.refresh();
        let menu = failed.menu();
        assert_eq!(visible_labels(&failed)[..2], ["Today", "---"]);
        assert!(!item(&menu, "Settings…").enabled);
        assert!(!item(&menu, "Sync Now").enabled);
    }

    #[test]
    fn app_names_keep_their_underscores() {
        let mut tray = tray();
        tray.callbacks.today = Box::new(|| {
            let mut today = demo_today()?;
            today.apps[0].app_name = "my_app__name_".into();
            Ok(today)
        });
        tray.refresh();
        assert_eq!(visible_labels(&tray)[1], "my__app____name__  6h 5m");
    }

    #[test]
    fn sync_requires_configuration_and_is_disabled_while_running() {
        let mut tray = tray();
        for (configured, running, label, enabled) in [
            (false, false, "Sync Now", false),
            (true, false, "Sync Now", true),
            (true, true, "Syncing…", false),
        ] {
            tray.callbacks.sync_state = Box::new(move || SyncView {
                configured,
                running,
                ..Default::default()
            });
            assert_eq!(item(&tray.menu(), label).enabled, enabled);
        }
    }

    #[test]
    fn icon_is_written_in_the_symbolic_theme_path_and_replaced_on_start() {
        let root = tempfile::tempdir().unwrap();
        let icons = root.path().join("spentday/icons");
        let svg = icons.join("hicolor/scalable/apps/spentday-symbolic.svg");
        write_icon(&icons).unwrap();
        assert_eq!(std::fs::read_to_string(&svg).unwrap(), ICON);
        let flat = icons.join("spentday-symbolic.svg");
        assert_eq!(std::fs::read_to_string(flat).unwrap(), ICON);
        std::fs::write(&svg, "old icon").unwrap();
        write_icon(&icons).unwrap();
        assert_eq!(std::fs::read_to_string(svg).unwrap(), ICON);
    }

    #[test]
    fn demo_has_its_own_item_id_and_opens_the_menu_on_left_click() {
        let mut tray = tray();
        assert_eq!(tray.id(), "day.spent.SpentDay");
        tray.demo = true;
        assert_eq!(tray.id(), "day.spent.SpentDay.Demo");
        assert_eq!(tray.icon_name(), "spentday-symbolic");
        assert_eq!(tray.icon_theme_path(), "/tmp/icons");
        const { assert!(<Tray as ksni::Tray>::MENU_ON_ACTIVATE) };
    }

    #[test]
    #[ignore = "needs a D-Bus session bus without a tray host; verify.sh runs it"]
    fn registers_when_the_watcher_starts_and_restarts() {
        use std::sync::mpsc;
        use zbus::blocking::connection::Builder;

        struct Watcher(mpsc::Sender<String>);

        #[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
        impl Watcher {
            fn register_status_notifier_item(&self, service: String) {
                self.0.send(service).unwrap();
            }
        }

        let handle = tray().assume_sni_available(true).spawn().unwrap();
        assert!(!handle.is_closed());
        let (registered, registrations) = mpsc::channel();
        let mut previous = None;
        for _ in 0..2 {
            let watcher = Builder::session()
                .unwrap()
                .serve_at("/StatusNotifierWatcher", Watcher(registered.clone()))
                .unwrap()
                .name("org.kde.StatusNotifierWatcher")
                .unwrap()
                .build()
                .unwrap();
            let name = registrations
                .recv_timeout(super::super::dbus::TIMEOUT)
                .unwrap();
            assert!(name.starts_with("org.kde.StatusNotifierItem-"));
            if let Some(previous) = previous {
                assert_eq!(name, previous);
            }
            previous = Some(name);
            watcher.close().unwrap();
        }
        handle.shutdown().wait();
    }
}
