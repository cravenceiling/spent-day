#![cfg(target_os = "linux")]

use std::process::Command;

#[test]
fn enable_autostart_uses_the_appimage_path_or_running_binary() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config");
    let binary = std::fs::canonicalize(env!("CARGO_BIN_EXE_spentday")).unwrap();
    let binary = binary.to_str().unwrap();
    let image = temp.path().join("Spent Day.AppImage");
    std::fs::write(&image, "").unwrap();
    let image = image.to_str().unwrap();
    for (appimage, appdir, expected) in [
        (None, None, binary),
        (Some("/opt/editor.AppImage"), Some("/opt/editor"), binary),
        (Some("/opt/editor.AppImage"), None, binary),
        (
            Some(image),
            Some(
                std::path::Path::new(binary)
                    .parent()
                    .unwrap()
                    .to_str()
                    .unwrap(),
            ),
            image,
        ),
    ] {
        let mut command = Command::new(binary);
        command
            .arg("--enable-autostart")
            .env("XDG_CONFIG_HOME", &config)
            .env_remove("APPIMAGE")
            .env_remove("APPDIR");
        if let Some(appimage) = appimage {
            command.env("APPIMAGE", appimage);
        }
        if let Some(appdir) = appdir {
            command.env("APPDIR", appdir);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let entry =
            std::fs::read_to_string(config.join("autostart/day.spent.SpentDay.desktop")).unwrap();
        let executable = entry
            .lines()
            .find_map(|line| line.strip_prefix("Exec=\""))
            .unwrap()
            .strip_suffix("\" --background")
            .unwrap();
        assert_eq!(
            std::fs::canonicalize(executable).unwrap(),
            std::fs::canonicalize(expected).unwrap()
        );
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("Settings > Launch at login")
        );
    }
    let output = Command::new(binary)
        .arg("--enable-autostart")
        .env("XDG_CONFIG_HOME", &config)
        .env("APPIMAGE", "/tmp/invalid%path")
        .env("APPDIR", std::path::Path::new(binary).parent().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Move Spent Day to another folder")
    );
}
