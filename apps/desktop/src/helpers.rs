use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

const GNOME: &[(&str, &str)] = &[
    (
        "metadata.json",
        include_str!("../linux/gnome-extension/metadata.json"),
    ),
    (
        "extension.js",
        include_str!("../linux/gnome-extension/extension.js"),
    ),
];
const KWIN: &[(&str, &str)] = &[
    (
        "metadata.json",
        include_str!("../linux/kwin-script/metadata.json"),
    ),
    (
        "contents/code/main.js",
        include_str!("../linux/kwin-script/contents/code/main.js"),
    ),
];

fn write_package(root: &Path, files: &[(&str, &str)]) -> io::Result<()> {
    for (name, contents) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, contents)?;
    }
    Ok(())
}

pub fn install() -> io::Result<()> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if desktop.contains("GNOME") {
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")))
            .ok_or_else(|| io::Error::other("HOME or XDG_DATA_HOME must be set"))?;
        write_package(
            &data.join("gnome-shell/extensions/spentday@spent.day"),
            GNOME,
        )?;
        if !Command::new("gnome-extensions")
            .args(["enable", "spentday@spent.day"])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            println!("Log out and back in, then run: gnome-extensions enable spentday@spent.day");
        }
    } else if desktop.contains("KDE") {
        // kpackagetool chooses the user's XDG data directory and handles existing installs.
        let package = tempfile::tempdir()?;
        write_package(package.path(), KWIN)?;
        if !Command::new("kpackagetool6")
            .args(["--type=KWin/Script", "--install"])
            .arg(package.path())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
            && !Command::new("kpackagetool6")
                .args(["--type=KWin/Script", "--upgrade"])
                .arg(package.path())
                .status()?
                .success()
        {
            return Err(io::Error::other(
                "kpackagetool6 could not install the KWin script",
            ));
        }
        if !Command::new("kwriteconfig6")
            .args([
                "--file",
                "kwinrc",
                "--group",
                "Plugins",
                "--key",
                "spentday-focusEnabled",
                "true",
            ])
            .status()?
            .success()
        {
            return Err(io::Error::other(
                "kwriteconfig6 could not enable the KWin script",
            ));
        }
        let _ = Command::new("qdbus6")
            .args([
                "org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.unloadScript",
                "spentday-focus",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !Command::new("qdbus6")
            .args(["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.start"])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            println!("Log out and back in to start the KWin script.");
        }
    } else {
        return Err(io::Error::other(
            "Run --install-helpers from a GNOME or KDE desktop session",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_packages_keep_the_installer_paths() {
        let home = tempfile::tempdir().unwrap();
        for data in [
            home.path().join(".local/share"),
            home.path().join("xdg-data"),
        ] {
            write_package(
                &data.join("gnome-shell/extensions/spentday@spent.day"),
                GNOME,
            )
            .unwrap();
            write_package(&data.join("kwin/scripts/spentday-focus"), KWIN).unwrap();
            for (path, expected) in [
                (
                    "gnome-shell/extensions/spentday@spent.day/metadata.json",
                    GNOME[0].1,
                ),
                (
                    "gnome-shell/extensions/spentday@spent.day/extension.js",
                    GNOME[1].1,
                ),
                ("kwin/scripts/spentday-focus/metadata.json", KWIN[0].1),
                (
                    "kwin/scripts/spentday-focus/contents/code/main.js",
                    KWIN[1].1,
                ),
            ] {
                assert_eq!(std::fs::read_to_string(data.join(path)).unwrap(), expected);
            }
        }
    }
}
