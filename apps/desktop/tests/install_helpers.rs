#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn installs_helpers_with_temporary_home_and_xdg_directories() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir(&bin).unwrap();
    for (name, script) in [
        ("gnome-extensions", "exit 1"),
        (
            "kpackagetool6",
            r#"printf '%s\n' "$2" >> "$HOME/calls"
dest="$XDG_DATA_HOME/kwin/scripts/spentday-focus"
if [ "$2" = --install ] && [ -d "$dest" ]; then exit 1; fi
mkdir -p "$dest"
cp -R "$3/metadata.json" "$3/contents" "$dest/""#,
        ),
        ("kwriteconfig6", r#"printf '%s\n' "$*" >> "$HOME/calls""#),
        (
            "qdbus6",
            r#"printf '%s\n' "$*" >> "$HOME/calls"
exit 1"#,
        ),
    ] {
        let path = bin.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    for xdg in [None, Some(temp.path().join("data"))] {
        let data = xdg.clone().unwrap_or_else(|| home.join(".local/share"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_spentday"));
        command
            .arg("--install-helpers")
            .env("HOME", &home)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env_remove("XDG_DATA_HOME");
        if let Some(xdg) = &xdg {
            command.env("XDG_DATA_HOME", xdg);
        }
        let output = command
            .env("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "Log out and back in, then run: gnome-extensions enable spentday@spent.day\n"
        );
        for (name, expected) in [
            (
                "metadata.json",
                include_str!("../linux/gnome-extension/metadata.json"),
            ),
            (
                "extension.js",
                include_str!("../linux/gnome-extension/extension.js"),
            ),
        ] {
            assert_eq!(
                std::fs::read_to_string(
                    data.join("gnome-shell/extensions/spentday@spent.day")
                        .join(name)
                )
                .unwrap(),
                expected
            );
        }
    }

    let data = temp.path().join("kde-data");
    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_spentday"))
            .arg("--install-helpers")
            .env("HOME", &home)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("XDG_DATA_HOME", &data)
            .env("XDG_CURRENT_DESKTOP", "KDE")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("Log out and back in to start the KWin script.")
        );
    }
    for (name, expected) in [
        (
            "metadata.json",
            include_str!("../linux/kwin-script/metadata.json"),
        ),
        (
            "contents/code/main.js",
            include_str!("../linux/kwin-script/contents/code/main.js"),
        ),
    ] {
        assert_eq!(
            std::fs::read_to_string(data.join("kwin/scripts/spentday-focus").join(name)).unwrap(),
            expected
        );
    }
    let calls = std::fs::read_to_string(home.join("calls")).unwrap();
    assert!(calls.contains("--upgrade\n"));
    assert!(calls.contains("--file kwinrc --group Plugins --key spentday-focusEnabled true\n"));
    assert!(
        calls.contains(
            "org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript spentday-focus\n"
        )
    );
    assert!(calls.contains("org.kde.KWin /Scripting org.kde.kwin.Scripting.start\n"));
}
