#![cfg(target_os = "linux")]

use std::process::Command;

#[test]
fn enable_autostart_uses_the_appimage_path_or_running_binary() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config");
    let binary = env!("CARGO_BIN_EXE_spentday");
    for appimage in [None, Some("/opt/Spent Day.AppImage")] {
        let mut command = Command::new(binary);
        command
            .arg("--enable-autostart")
            .env("XDG_CONFIG_HOME", &config)
            .env_remove("APPIMAGE");
        if let Some(appimage) = appimage {
            command.env("APPIMAGE", appimage);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let entry =
            std::fs::read_to_string(config.join("autostart/day.spent.SpentDay.desktop")).unwrap();
        assert!(entry.contains(&format!(
            "Exec=\"{}\" --background\n",
            appimage.unwrap_or(binary)
        )));
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
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Move Spent Day to another folder")
    );
}
