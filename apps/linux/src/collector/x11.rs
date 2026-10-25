use super::{Collector, Window};
use std::fs;
use std::path::Path;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
use x11rb::rust_connection::RustConnection;

x11rb::atom_manager! {
    Atoms: AtomsCookie { _NET_ACTIVE_WINDOW, _NET_WM_PID, }
}

pub struct X11 {
    conn: RustConnection,
    root: u32,
    atoms: Atoms,
}

impl X11 {
    pub fn connect() -> Result<X11, String> {
        let (conn, screen) = RustConnection::connect(None).map_err(|e| format!("X11: {e}"))?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        Ok(X11 { conn, root, atoms })
    }

    fn property(&self, window: u32, name: impl Into<u32>, kind: impl Into<u32>) -> Option<Vec<u8>> {
        let reply = self.conn.get_property(false, window, name, kind, 0, 4096).ok()?.reply().ok()?;
        Some(reply.value)
    }

    fn pid(&self, window: u32) -> Option<u32> {
        let reply = self.conn.get_property(false, window, self.atoms._NET_WM_PID, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?;
        reply.value32()?.next()
    }
}

impl Collector for X11 {
    fn name(&self) -> &'static str {
        "x11"
    }

    fn active(&mut self) -> Result<Option<Window>, String> {
        let reply = self
            .conn
            .get_property(false, self.root, self.atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let window = reply.value32().and_then(|mut v| v.next()).unwrap_or(0);
        if window == 0 {
            return Ok(None);
        }
        // The window can vanish between the two requests, so a failed property read means "none".
        // Some toolkits set no WM_CLASS, and then the process name stands in for the app.
        let app_id = self
            .property(window, AtomEnum::WM_CLASS, AtomEnum::STRING)
            .and_then(|v| app_id(&v))
            .or_else(|| self.pid(window).and_then(|pid| process_name(Path::new("/proc"), pid)));
        Ok(app_id.and_then(|id| Window::new(&id)))
    }
}

/// `WM_CLASS` is `instance\0class\0`. The class is the app's identity (`firefox`, `XTerm`,
/// `org.gnome.Nautilus`), but when it only differs from the instance by case
/// (`Google-chrome` / `google-chrome`) the lowercase instance matches the Wayland app id.
fn app_id(wm_class: &[u8]) -> Option<String> {
    let mut parts = wm_class.split(|&b| b == 0).map(|p| String::from_utf8_lossy(p).into_owned());
    let instance = parts.next().unwrap_or_default();
    let class = parts.next().filter(|c| !c.is_empty()).unwrap_or_else(|| instance.clone());
    if class.is_empty() {
        return None;
    }
    Some(if class != instance && class.to_lowercase() == instance { instance } else { class })
}

/// `/proc/<pid>/comm`, the name the kernel keeps for a process, cut to 15 bytes.
fn process_name(proc: &Path, pid: u32) -> Option<String> {
    let comm = fs::read_to_string(proc.join(pid.to_string()).join("comm")).ok()?;
    Some(comm.trim_end().to_owned()).filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::{CreateWindowAux, PropMode, WindowClass};
    use x11rb::wrapper::ConnectionExt as _;

    #[test]
    fn process_name_from_proc() {
        let proc = tempfile::tempdir().unwrap();
        fs::create_dir_all(proc.path().join("4242")).unwrap();
        fs::write(proc.path().join("4242/comm"), "inkscape\n").unwrap();
        assert_eq!(process_name(proc.path(), 4242).as_deref(), Some("inkscape"));
        assert_eq!(process_name(proc.path(), 4243), None);
    }

    /// A window with `_NET_WM_PID` but no `WM_CLASS`, made active by setting the root property
    /// directly, as a window manager would.
    #[test]
    #[ignore = "needs an X server; verify.sh runs it"]
    fn window_without_wm_class_uses_its_process_name() {
        let mut x11 = X11::connect().unwrap();
        let window = x11.conn.generate_id().unwrap();
        x11.conn
            .create_window(0, window, x11.root, 0, 0, 10, 10, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())
            .unwrap();
        x11.conn.change_property32(PropMode::REPLACE, window, x11.atoms._NET_WM_PID, AtomEnum::CARDINAL, &[std::process::id()]).unwrap();
        x11.conn.change_property32(PropMode::REPLACE, x11.root, x11.atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW, &[window]).unwrap();
        x11.conn.sync().unwrap();
        let comm = fs::read_to_string("/proc/self/comm").unwrap().trim_end().to_owned();
        assert_eq!(x11.active().unwrap(), Some(Window { app_id: comm }));
    }

    #[test]
    fn wm_class_to_app_id() {
        assert_eq!(app_id(b"google-chrome\0Google-chrome\0").as_deref(), Some("google-chrome"));
        assert_eq!(app_id(b"chromium\0Chromium\0").as_deref(), Some("chromium"));
        assert_eq!(app_id(b"Navigator\0firefox\0").as_deref(), Some("firefox"));
        assert_eq!(app_id(b"xterm\0XTerm\0").as_deref(), Some("xterm"));
        assert_eq!(app_id(b"spentday-a\0SpentDayA\0").as_deref(), Some("SpentDayA"));
        assert_eq!(app_id(b"org.gnome.Nautilus\0org.gnome.Nautilus\0").as_deref(), Some("org.gnome.Nautilus"));
        assert_eq!(app_id(b"\0\0"), None);
    }
}
