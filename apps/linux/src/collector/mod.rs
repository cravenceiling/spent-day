//! Which window is in front, per display server. One implementation each, picked at startup.

mod gnome;
mod hyprland;
mod kde;
mod sway;
mod x11;

use std::env;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// The bus name Spent Day holds for the GNOME extension and the KWin script. The extension
/// answers only its owner, and KWin scripts report to it. `take_focus_name` takes it so that no
/// other connection can replace Spent Day while it runs.
const FOCUS_NAME: &str = "day.spent.SpentDay.Focus";

/// Takes `FOCUS_NAME` without letting another connection replace this one, and without waiting
/// in line for it. zbus's `Builder::name` would allow both replacing and being replaced.
fn take_focus_name(conn: &zbus::blocking::Connection) -> Result<(), String> {
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    match conn.request_name_with_flags(FOCUS_NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(()),
        Ok(_) | Err(zbus::Error::NameTaken) => Err(format!(
            "Another program holds the session bus name {FOCUS_NAME}. Quit any other copy of Spent Day, then start it again."
        )),
        Err(e) => Err(format!("Could not take the session bus name {FOCUS_NAME}. {e}")),
    }
}

/// The app in front. Window titles are never read: a private browser window
/// shows its page title there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub app_id: String,
}

impl Window {
    fn new(app_id: &str) -> Option<Window> {
        (!app_id.is_empty()).then(|| Window { app_id: app_id.to_owned() })
    }
}

pub trait Collector: Send {
    fn name(&self) -> &'static str;
    /// `Ok(None)` when no window has focus (empty workspace, desktop).
    fn active(&mut self) -> Result<Option<Window>, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    X11,
    Sway,
    Hyprland,
    Gnome,
    Kde,
}

impl Kind {
    fn parse(name: &str) -> Option<Kind> {
        Some(match name {
            "x11" => Kind::X11,
            "sway" => Kind::Sway,
            "hyprland" => Kind::Hyprland,
            "gnome" => Kind::Gnome,
            "kde" => Kind::Kde,
            _ => return None,
        })
    }

    /// `SPENTDAY_COLLECTOR` wins. Otherwise the session's own variables decide.
    pub fn detect() -> Option<Kind> {
        if let Some(kind) = env::var("SPENTDAY_COLLECTOR").ok().as_deref().and_then(Kind::parse) {
            return Some(kind);
        }
        let set = |name: &str| env::var_os(name).is_some_and(|v| !v.is_empty());
        if !set("WAYLAND_DISPLAY") {
            return set("DISPLAY").then_some(Kind::X11);
        }
        if set("SWAYSOCK") {
            return Some(Kind::Sway);
        }
        if set("HYPRLAND_INSTANCE_SIGNATURE") {
            return Some(Kind::Hyprland);
        }
        let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_uppercase();
        if desktop.split(':').any(|d| d == "GNOME") {
            return Some(Kind::Gnome);
        }
        if desktop.split(':').any(|d| d == "KDE") {
            return Some(Kind::Kde);
        }
        None
    }

    pub fn open(self) -> Result<Box<dyn Collector>, String> {
        Ok(match self {
            Kind::X11 => Box::new(x11::X11::connect()?),
            Kind::Sway => Box::new(sway::Sway),
            Kind::Hyprland => Box::new(hyprland::Hyprland),
            Kind::Gnome => Box::new(gnome::Gnome::connect()?),
            Kind::Kde => Box::new(kde::Kde::serve()?),
        })
    }
}

/// How long `swaymsg` or `hyprctl` may take before it is killed, so a compositor that stops
/// answering cannot stall the tracker.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(1);

fn run_json(program: &str, args: &[&str]) -> Result<serde_json::Value, String> {
    let mut child = Command::new(program)
        .args(args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    // Read on another thread, so a large tree cannot fill the pipe while this one waits.
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (read, output) = mpsc::channel();
    thread::spawn(move || {
        let mut out = Vec::new();
        let _ = read.send(stdout.read_to_end(&mut out).map(|_| out));
    });
    // A process the command started can hold the pipe open after the command exits.
    let out = match output.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(out) => out.map_err(|e| format!("{program}: {e}"))?,
        Err(RecvTimeoutError::Timeout) => return Err(kill(program, child)),
        Err(RecvTimeoutError::Disconnected) => return Err(format!("{program}: reader panicked")),
    };
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("{program}: {e}"))? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err(kill(program, child));
        }
        thread::sleep(Duration::from_millis(5));
    };
    if !status.success() {
        return Err(format!("{program} exited with {status}"));
    }
    serde_json::from_slice(&out).map_err(|e| format!("{program}: {e}"))
}

/// Kills the command and every process it started, which share its process group. The command
/// is not reaped yet, so its pid, and with it the group id, cannot have been reused.
fn kill(program: &str, mut child: Child) -> String {
    // SAFETY: kill(2) takes no pointers. A negative pid names the process group.
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let _ = child.wait();
    format!("{program} did not answer within {} s", COMMAND_TIMEOUT.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_large_output() {
        let value = run_json("sh", &["-c", "printf '{\"pad\": \"'; head -c 300000 /dev/zero | tr '\\0' x; printf '\"}'"]).unwrap();
        assert_eq!(value["pad"].as_str().map(str::len), Some(300_000));
    }

    #[test]
    fn kills_a_command_that_hangs() {
        let started = Instant::now();
        assert_eq!(run_json("sh", &["-c", "sleep 30"]), Err("sh did not answer within 1 s".to_owned()));
        assert!(started.elapsed() < Duration::from_secs(3), "took {:?}", started.elapsed());
    }

    #[test]
    fn kills_a_command_whose_child_holds_its_output_open() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 4 & echo $! > {}; printf '{{}}'", pid_file.display());
        let started = Instant::now();
        assert_eq!(run_json("sh", &["-c", &script]), Err("sh did not answer within 1 s".to_owned()));
        assert!(started.elapsed() < Duration::from_millis(1200), "took {:?}", started.elapsed());
        let stat = format!("/proc/{}/stat", fs::read_to_string(&pid_file).unwrap().trim());
        let gone = || fs::read_to_string(&stat).map_or(true, |state| state.contains(") Z "));
        let killed_by = Instant::now() + Duration::from_millis(500);
        while !gone() && Instant::now() < killed_by {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(gone(), "the command's child still runs");
    }

    #[test]
    fn reports_a_failing_command() {
        assert_eq!(run_json("sh", &["-c", "exit 3"]), Err("sh exited with exit status: 3".to_owned()));
    }
}
