use super::{Collector, Window, take_focus_name};
use crate::platform::linux::dbus;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::{env, fs};
use zbus::blocking::{Connection, connection::Builder};

/// The bundled KWin script (`kwin-script/`) calls `Report` on every focus change.
/// KWin scripts cannot answer calls, so on startup Spent Day loads a one-shot script that reports
/// the window active right now. Reports from any connection but KWin's are turned down.
pub struct Kde {
    _conn: Connection,
    focused: Arc<Mutex<Option<Window>>>,
}

struct Sink {
    focused: Arc<Mutex<Option<Window>>>,
    reported: Sender<()>,
}

#[zbus::interface(name = "day.spent.SpentDay.Focus")]
impl Sink {
    async fn report(
        &self,
        app_id: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        if !from_kwin(conn, &header).await {
            return Err(zbus::fdo::Error::AccessDenied(
                "only KWin may report focus".into(),
            ));
        }
        *self.focused.lock().unwrap() = Window::new(&app_id);
        let _ = self.reported.send(());
        Ok(())
    }
}

/// Whether the call came from the connection that owns `org.kde.KWin`. KWin scripts call out on
/// KWin's own connection, so another process can send a report but cannot pass as KWin.
async fn from_kwin(conn: &zbus::Connection, header: &zbus::message::Header<'_>) -> bool {
    let Some(sender) = header.sender() else {
        return false;
    };
    let owner = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &("org.kde.KWin",),
        )
        .await
        .and_then(|reply| reply.body().deserialize::<String>());
    owner.is_ok_and(|owner| owner == sender.as_str())
}

/// KWin loads a script name once, so a restart unloads the previous run's script first.
const REPORT_NOW_NAME: &str = "spentday-focus-now";
const REPORT_NOW: &str = r#"const window = workspace.activeWindow;
callDBus("day.spent.SpentDay.Focus", "/day/spent/SpentDay/Focus", "day.spent.SpentDay.Focus", "Report",
         window ? String(window.resourceClass) : "");
"#;

impl Kde {
    pub fn serve() -> Result<Kde, String> {
        let focused = Arc::new(Mutex::new(None));
        let (reported, report) = mpsc::channel();
        let conn = Builder::session()
            .map(|b| b.method_timeout(dbus::TIMEOUT))
            .and_then(|b| {
                b.serve_at(
                    "/day/spent/SpentDay/Focus",
                    Sink {
                        focused: focused.clone(),
                        reported,
                    },
                )
            })
            .and_then(|b| b.build())
            .map_err(|e| e.to_string())?;
        take_focus_name(&conn)?;
        if let Err(e) = report_now(&conn, &report) {
            eprintln!(
                "spentday: could not ask KWin for the active window, it is known at the next focus change: {e}"
            );
        }
        Ok(Kde {
            _conn: conn,
            focused,
        })
    }
}

/// Loads the one-shot script, and once it has reported, unloads it and deletes its file.
fn report_now(conn: &Connection, report: &Receiver<()>) -> Result<(), String> {
    // Without XDG_RUNTIME_DIR the base is the shared temp dir, where another user could plant a
    // symlink or their own script at a fixed name.
    let base = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    let dir = tempfile::Builder::new()
        .prefix("spentday-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&base)
        .map_err(|e| format!("{}: {e}", base.display()))?;
    let mut file = tempfile::Builder::new()
        .prefix(REPORT_NOW_NAME)
        .suffix(".js")
        .tempfile_in(dir.path())
        .map_err(|e| format!("{}: {e}", dir.path().display()))?;
    file.write_all(REPORT_NOW.as_bytes())
        .map_err(|e| format!("{}: {e}", file.path().display()))?;
    let scripting = dbus::proxy(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting")
        .map_err(|e| e.to_string())?;
    let _: zbus::Result<bool> = scripting.call("unloadScript", &(REPORT_NOW_NAME));
    let id: i32 = scripting
        .call(
            "loadScript",
            &(file.path().to_string_lossy().as_ref(), REPORT_NOW_NAME),
        )
        .map_err(|e| e.to_string())?;
    if id < 0 {
        return Err(format!("KWin did not load {}", file.path().display()));
    }
    scripting
        .call::<_, _, ()>("start", &())
        .map_err(|e| e.to_string())?;
    let _ = report.recv_timeout(dbus::TIMEOUT);
    let _: zbus::Result<bool> = scripting.call("unloadScript", &(REPORT_NOW_NAME));
    Ok(())
}

impl Collector for Kde {
    fn name(&self) -> &'static str {
        "kde"
    }

    fn active(&mut self) -> Result<Option<Window>, String> {
        Ok(self.focused.lock().unwrap().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::collector::FOCUS_NAME;
    use std::path::Path;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};

    /// Records what Spent Day asks of KWin's scripting service.
    #[derive(Default, Clone)]
    struct Calls(Arc<Mutex<Vec<String>>>);

    struct Scripting(Calls, Arc<Mutex<Option<String>>>);

    #[zbus::interface(name = "org.kde.kwin.Scripting")]
    impl Scripting {
        #[zbus(name = "unloadScript")]
        fn unload_script(&self, name: String) -> bool {
            self.0.0.lock().unwrap().push(format!("unload {name}"));
            false
        }

        /// Records whether the script asks for the active window, and whether it sits in a fresh
        /// directory only this user can open.
        #[zbus(name = "loadScript")]
        fn load_script(&self, path: String, name: String) -> i32 {
            let script = fs::read_to_string(&path).unwrap_or_default();
            let reads_active =
                script.contains("workspace.activeWindow") && script.contains("\"Report\"");
            let dir = Path::new(&path).parent().unwrap();
            let base = env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(env::temp_dir);
            let private = dir != base
                && fs::symlink_metadata(dir).unwrap().permissions().mode() & 0o777 == 0o700;
            self.0.0.lock().unwrap().push(format!(
                "load {name} reads_active={reads_active} private={private}"
            ));
            *self.1.lock().unwrap() = Some(path);
            7
        }

        /// Runs the script as KWin would, which reports the active window on KWin's connection.
        #[zbus(name = "start")]
        async fn start(&self, #[zbus(connection)] conn: &zbus::Connection) {
            self.0.0.lock().unwrap().push("start".into());
            conn.call_method(
                Some(FOCUS_NAME),
                "/day/spent/SpentDay/Focus",
                Some(FOCUS_NAME),
                "Report",
                &("org.kde.dolphin",),
            )
            .await
            .unwrap();
        }
    }

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn asks_kwin_for_the_active_window_on_start() {
        // Another user may have left a symlink at the old fixed path in the shared temp dir.
        let victim = tempfile::NamedTempFile::new().unwrap();
        let trap = env::temp_dir().join("spentday-focus-now.js");
        let _ = fs::remove_file(&trap);
        std::os::unix::fs::symlink(victim.path(), &trap).unwrap();

        let calls = Calls::default();
        let loaded = Arc::new(Mutex::new(None));
        let _kwin = Builder::session()
            .unwrap()
            .name("org.kde.KWin")
            .unwrap()
            .serve_at("/Scripting", Scripting(calls.clone(), loaded.clone()))
            .unwrap()
            .build()
            .unwrap();
        let mut kde = Kde::serve().unwrap();
        let _ = fs::remove_file(&trap);
        assert_eq!(
            fs::read_to_string(victim.path()).unwrap(),
            "",
            "wrote through the symlink"
        );
        assert_eq!(
            *calls.0.lock().unwrap(),
            [
                "unload spentday-focus-now",
                "load spentday-focus-now reads_active=true private=true",
                "start",
                "unload spentday-focus-now"
            ]
        );
        assert_eq!(
            kde.active().unwrap(),
            Some(Window {
                app_id: "org.kde.dolphin".into()
            })
        );
        let script = loaded.lock().unwrap().clone().unwrap();
        assert!(!Path::new(&script).exists(), "{script} is still there");

        // Any other process on the session bus is turned down.
        let other = Connection::session().unwrap();
        let focus =
            dbus::proxy(&other, FOCUS_NAME, "/day/spent/SpentDay/Focus", FOCUS_NAME).unwrap();
        let denied = focus
            .call::<_, _, ()>("Report", &("evil.app",))
            .unwrap_err();
        assert!(denied.to_string().contains("AccessDenied"), "{denied}");
        assert_eq!(
            kde.active().unwrap(),
            Some(Window {
                app_id: "org.kde.dolphin".into()
            })
        );

        // Nobody can take the name from a running Spent Day, and a second one does not start.
        let reply = other.request_name_with_flags(
            FOCUS_NAME,
            RequestNameFlags::ReplaceExisting | RequestNameFlags::DoNotQueue,
        );
        assert!(
            matches!(
                reply,
                Ok(RequestNameReply::Exists) | Err(zbus::Error::NameTaken)
            ),
            "{reply:?}"
        );
        let second = Kde::serve().err().unwrap();
        assert!(second.contains("Another program holds"), "{second}");
    }
}
