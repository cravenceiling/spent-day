use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_ID: &str = "day.spent.SpentDay";
const TEMPLATE: &str = include_str!("../../../linux/data/day.spent.SpentDay-autostart.desktop");

fn config_dir() -> Result<PathBuf, String> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty() && Path::new(path).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))
        .ok_or_else(|| "Cannot find the config folder. Set HOME or XDG_CONFIG_HOME.".into())
}

pub fn autostart() -> Option<bool> {
    Some(config_dir().is_ok_and(|config| enabled(&config)))
}

fn service_enabled(config: &Path) -> bool {
    // Enablement is recorded on disk, so this also works without a running user manager.
    std::fs::read_dir(config.join("systemd/user"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".wants"))
        .any(|entry| entry.path().join("spentday.service").is_file())
}

fn enabled(config: &Path) -> bool {
    let path = config.join("autostart/day.spent.SpentDay.desktop");
    service_enabled(config)
        || (path.exists()
            && !std::fs::read_to_string(path).is_ok_and(|entry| {
                entry.lines().any(|line| {
                    matches!(
                        line.trim(),
                        "Hidden=true" | "X-GNOME-Autostart-enabled=false"
                    )
                })
            }))
}

pub fn set_autostart(on: bool) -> Result<Option<String>, String> {
    let config = config_dir()?;
    if service_enabled(&config) {
        if on {
            return Ok(None);
        }
        let output = Command::new("systemctl")
            .args(["--user", "disable", "spentday.service"])
            .output()
            .map_err(|error| format!("Could not disable spentday.service: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Could not disable spentday.service: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    let binary = if on {
        let executable = std::env::current_exe()
            .map_err(|error| format!("Cannot find the running binary: {error}"))?;
        // Linux keeps the old inode running after a package replaces the executable.
        let bytes = executable.as_os_str().as_bytes();
        let executable = PathBuf::from(std::ffi::OsStr::from_bytes(
            bytes.strip_suffix(b" (deleted)").unwrap_or(bytes),
        ));
        let appdir = std::env::var_os("APPDIR")
            .filter(|path| !path.is_empty())
            .and_then(|path| std::fs::canonicalize(path).ok());
        Some(
            std::env::var_os("APPIMAGE")
                .filter(|path| !path.is_empty())
                .filter(|_| appdir.is_some_and(|dir| executable.starts_with(dir)))
                .map(PathBuf::from)
                .unwrap_or(executable),
        )
    } else {
        None
    };
    set_entry(&config, binary.as_deref())?;
    Ok(None)
}

fn set_entry(config: &Path, binary: Option<&Path>) -> Result<(), String> {
    let directory = config.join("autostart");
    let path = directory.join("day.spent.SpentDay.desktop");
    let result = (|| -> std::io::Result<()> {
        if let Some(binary) = binary {
            let binary = binary.to_str().filter(|binary| {
                Path::new(binary).is_absolute()
                    && !binary.chars().any(|c| {
                        c.is_control() || matches!(c, '%' | '$' | '"' | '\'' | '\\' | '`')
                    })
            }).ok_or_else(|| std::io::Error::other(
                "The app path must be absolute and cannot contain %, $, quotes, backslashes, backticks or control characters. Move Spent Day to another folder.",
            ))?;
            let entry = TEMPLATE.replace(
                "Exec=spentday --background",
                &format!("Exec=\"{binary}\" --background"),
            );
            std::fs::create_dir_all(&directory)?;
            let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
            temporary.write_all(entry.as_bytes())?;
            temporary.persist(&path).map_err(|error| error.error)?;
        } else {
            match std::fs::read_to_string(&path) {
                Ok(entry) if entry.contains(APP_ID) => std::fs::remove_file(&path)?,
                Ok(_) => {
                    return Err(std::io::Error::other(
                        "The autostart entry belongs to another app. Remove it manually to turn off launch at login.",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })();
    result.map_err(|error| format!("Could not update launch at login: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_round_trip_and_hidden_entry() {
        let config = tempfile::tempdir().unwrap();
        let config = config.path();
        assert!(!enabled(config));
        set_entry(config, Some(Path::new("/opt/Spent Day/spentday"))).unwrap();
        assert!(enabled(config));
        let path = config.join("autostart/day.spent.SpentDay.desktop");
        let entry = std::fs::read_to_string(&path).unwrap();
        assert!(entry.contains("Exec=\"/opt/Spent Day/spentday\" --background\n"));
        for disabled in ["Hidden=true", "X-GNOME-Autostart-enabled=false"] {
            std::fs::write(&path, format!("{entry}{disabled}\n")).unwrap();
            assert!(!enabled(config));
        }
        set_entry(config, Some(Path::new("/opt/new.AppImage"))).unwrap();
        assert!(enabled(config));
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        set_entry(config, None).unwrap();
        set_entry(config, None).unwrap();
        assert!(!enabled(config));
    }

    #[test]
    fn foreign_entry_is_not_removed() {
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("autostart/day.spent.SpentDay.desktop");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[Desktop Entry]\nExec=another-app\n").unwrap();
        assert!(
            set_entry(config.path(), None)
                .unwrap_err()
                .contains("another app")
        );
        assert!(path.exists());
    }

    #[test]
    fn unsupported_binary_paths_are_refused() {
        let config = tempfile::tempdir().unwrap();
        for bad in [
            "relative", "/tmp/%", "/tmp/$", "/tmp/\"", "/tmp/'", "/tmp/\\", "/tmp/`", "/tmp/\n",
            "/tmp/\t",
        ] {
            assert!(
                set_entry(config.path(), Some(Path::new(bad))).is_err(),
                "{bad:?}"
            );
        }
        assert!(!config.path().join("autostart").exists());
    }

    #[test]
    fn enabled_service_counts_even_when_entry_is_hidden() {
        use std::os::unix::fs::symlink;
        let config = tempfile::tempdir().unwrap();
        let wants = config
            .path()
            .join("systemd/user/graphical-session.target.wants");
        std::fs::create_dir_all(&wants).unwrap();
        let link = wants.join("spentday.service");
        let unit = config.path().join("systemd/user/spentday.service");
        symlink(&unit, &link).unwrap();
        assert!(!enabled(config.path()));
        std::fs::write(&unit, "[Service]\nExecStart=/usr/bin/spentday\n").unwrap();
        let entry = config.path().join("autostart/day.spent.SpentDay.desktop");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(entry, "Hidden=true\n").unwrap();
        assert!(enabled(config.path()));
        std::fs::remove_file(link).unwrap();
        assert!(!enabled(config.path()));
    }
}
