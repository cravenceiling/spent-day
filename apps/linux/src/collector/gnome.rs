use super::{Collector, Window, take_focus_name};
use crate::dbus;
use zbus::blocking::Proxy;

/// Asks the bundled GNOME Shell extension (`gnome-extension/`), since Mutter does not expose
/// the focused window to clients on Wayland. The extension answers only the owner of
/// `day.spent.SpentDay.Focus`, so this connection takes that name first.
pub struct Gnome {
    proxy: Proxy<'static>,
}

impl Gnome {
    pub fn connect() -> Result<Gnome, String> {
        let conn = dbus::session().map_err(|e| e.to_string())?;
        take_focus_name(&conn)?;
        let proxy = dbus::proxy(&conn, "org.gnome.Shell", "/day/spent/SpentDay/Focus", "day.spent.SpentDay.Focus")
            .map_err(|e| e.to_string())?;
        Ok(Gnome { proxy })
    }
}

impl Collector for Gnome {
    fn name(&self) -> &'static str {
        "gnome"
    }

    fn active(&mut self) -> Result<Option<Window>, String> {
        let app_id: String = self
            .proxy
            .call("Get", &())
            .map_err(|e| format!("GNOME Shell extension spentday@spent.day is not answering: {e}"))?;
        Ok(Window::new(&app_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::FOCUS_NAME;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};

    #[test]
    #[ignore = "needs a D-Bus session bus; verify.sh runs it"]
    fn keeps_the_focus_name_while_it_runs() {
        let _gnome = Gnome::connect().unwrap();
        let other = zbus::blocking::Connection::session().unwrap();
        let reply = other.request_name_with_flags(FOCUS_NAME, RequestNameFlags::ReplaceExisting | RequestNameFlags::DoNotQueue);
        assert!(matches!(reply, Ok(RequestNameReply::Exists) | Err(zbus::Error::NameTaken)), "{reply:?}");
        let second = Gnome::connect().err().unwrap();
        assert!(second.contains("Another program holds"), "{second}");
    }
}
