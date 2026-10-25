# Testing

## What has been tested

Spent Day is early and unreleased. Each platform has been tested on real builds:

| Platform | Tested on | Not tested yet |
|---|---|---|
| macOS | macOS 26: app switches, idle, Chrome incognito | Brave, Helium, Edge, Vivaldi, Opera and Arc live; the lock screen |
| Linux | Docker: X11, headless Sway, Chromium with the extension. A Debian 13 VM with Xfce: app switches, idle, autostart, the extension and sync | A real GNOME, KDE or Hyprland session |
| Android | Emulator (Android 16): Chrome and Firefox, including private tabs | Real phones, Android 8 to 15 |

## Checks

Run these from the repository root:

```sh
cargo test --workspace                  # core, server, CLI
apps/macos/test.sh                      # macOS collector against the real core
apps/linux/verify.sh                    # Linux end to end in Docker
node --test extension/test/*.test.mjs   # extension
(cd apps/android && ./gradlew testDebugUnitTest)
```

`scripts/e2e-cross-device.sh` syncs the three real apps through a local server.
