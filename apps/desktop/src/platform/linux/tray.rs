use super::paths;
use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};
use std::path::{Path, PathBuf};

const ICON: &str = include_str!("../../../linux/data/icons/spentday-symbolic.svg");

pub struct TrayCallbacks {
    pub open: Box<dyn Fn(String) + Send + Sync>,
    pub settings: Box<dyn Fn(String) + Send + Sync>,
    pub settings_available: Box<dyn Fn() -> bool + Send + Sync>,
    pub quit: Box<dyn Fn() + Send + Sync>,
}

struct Tray {
    demo: bool,
    icons: String,
    callbacks: TrayCallbacks,
}

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
        Tray {
            demo,
            icons,
            callbacks,
        }
        // Autostart can precede Waybar; ksni keeps exporting and registers when it appears.
        .assume_sni_available(true)
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

impl ksni::Tray for Tray {
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

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.callbacks.open)(activation_token());
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open Spent Day".into(),
                activate: Box::new(|tray: &mut Self| {
                    (tray.callbacks.open)(activation_token());
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings".into(),
                enabled: (self.callbacks.settings_available)(),
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
        ]
    }

    // ksni rebuilds the menu on AboutToShow only when this is overridden, so Settings picks up a
    // startup failure that happened after the host cached the menu.
    fn menu_about_to_show(&mut self) {}

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        eprintln!("spentday: no system tray host; tracking continues");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;

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
    fn demo_has_its_own_item_id_and_failed_startup_disables_settings() {
        let mut tray = Tray {
            demo: false,
            icons: "/tmp/icons".into(),
            callbacks: TrayCallbacks {
                open: Box::new(|_| {}),
                settings: Box::new(|_| {}),
                settings_available: Box::new(|| false),
                quit: Box::new(|| {}),
            },
        };
        assert_eq!(tray.id(), "day.spent.SpentDay");
        tray.demo = true;
        assert_eq!(tray.id(), "day.spent.SpentDay.Demo");
        assert_eq!(tray.icon_name(), "spentday-symbolic");
        assert_eq!(tray.icon_theme_path(), "/tmp/icons");
        let MenuItem::Standard(settings) = tray.menu().remove(1) else {
            panic!("missing Settings item")
        };
        assert!(!settings.enabled);
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

        let handle = Tray {
            demo: true,
            icons: String::new(),
            callbacks: TrayCallbacks {
                open: Box::new(|_| {}),
                settings: Box::new(|_| {}),
                settings_available: Box::new(|| true),
                quit: Box::new(|| {}),
            },
        }
        .assume_sni_available(true)
        .spawn()
        .unwrap();
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
