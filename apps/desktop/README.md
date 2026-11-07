# Spent Day desktop

One Rust app on [Rune](https://github.com/anomalyco/rune) for macOS and Linux, linked directly to `spentday-core`. The desktop crate has its own workspace and lockfile so server and CLI builds do not compile Rune.

## Build and install

Use Rust stable and a C compiler. Rune is pinned in `Cargo.toml` and `Cargo.lock`.

On Apple Silicon, install Apple's Command Line Tools. The app runs on macOS 14 or later. From the repository root:

```sh
apps/desktop/macos/make-dev-cert.sh   # optional, see Signing
apps/desktop/macos/bundle.sh
apps/desktop/macos/install.sh
```

`bundle.sh` builds `apps/desktop/macos/build/Spent Day.app`. `install.sh` copies it to `/Applications` and opens it. The scripts target arm64; Intel Macs are not supported yet.

On Debian 13 or Ubuntu with recent Mesa:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libxkbcommon-x11-0 libx11-6 libx11-xcb1 libxcursor1 libxi6 mesa-vulkan-drivers libvulkan1
apps/desktop/linux/install.sh
# Or install a systemd user service instead of an autostart entry:
apps/desktop/linux/install.sh --systemd
```

Linux needs a Vulkan driver. Mesa lavapipe supports machines without a GPU; use trixie or newer for headless renders. Arch build packages are `base-devel git rust libxkbcommon libxkbcommon-x11 wayland vulkan-icd-loader libx11 libxcursor libxi hicolor-icon-theme`. Install a Vulkan driver for the machine too.

`PREFIX` changes the Linux install location from `~/.local` and must be absolute. After helper updates, log out and back in to reload the GNOME extension or KWin script. On GNOME enable `spentday@spent.day`; on KDE enable Spent Day Focus under System Settings > Window Management > KWin Scripts.

For an unbundled development build:

```sh
cd apps/desktop
cargo build --locked
./target/debug/spentday --demo --dashboard
```

## Launch and quit

| Argument | Effect |
|---|---|
| No launch flag | macOS stays in the menu bar, except for first-launch onboarding. Linux opens the dashboard. |
| `--background`, `-b` | Track without opening the dashboard. |
| `--dashboard` | Open the dashboard. |
| `--settings` | Open the dashboard with its Settings overlay. |
| `--day YYYY-MM-DD` | Select a local date. A missing or invalid date falls back to today. |
| `--demo` | Use fixed sample data, without opening the database, tracking, listening for the extension or auto syncing. |

The last launch flag wins. Arguments the app does not consume go to Rune, including `--snap`, `--scene`, `--window` and `--tree`.

Closing the dashboard keeps tracking and retains the selected day and drill. Quit Spent Day or Cmd/Ctrl+Q stops the process and flushes the current span. SIGTERM and SIGINT use the same shutdown path. A startup failure opens a fatal window even in background mode; dismissing it exits with code 3. A missing display logs an error and exits with code 1.

A second Linux launch forwards its activation token and whether to show the dashboard to the running process. Demo uses a separate instance name. On macOS, reopening the app opens the retained dashboard.

The dashboard refreshes every 30 seconds. Automatic sync runs after 10 seconds, then every 5 minutes. Demo data covers 28 October 2026 and four earlier Wednesdays; other days are empty.

## Day view

The headline counts overlapping screens once; each device shows its own total. Details lists simultaneous device time, longest stretches, time after 23:00 and each device's apps and sites. Browser time belongs to its site or the browser app, without counting both.

Use the header arrows or Cmd/Ctrl+Left and Right to change days. Next stops at today. A dashboard showing today follows midnight; a past day stays selected. The comparison needs at least two of the previous four matching weekdays with data. Today compares only up to the current local time.

Click a device label or tile to drill into sessions. Breadcrumbs and Escape go back. Tab and arrows move focus; Return and Space activate tiles. Hover shows sessions after 300 ms, with immediate changes between tiles. Items below 0.3% of the day total share an Other tile.

Appearance and reduced motion follow the system. App colours come from installed icons; sites and missing icons use stable colours. No favicons are fetched. Labels appear only when the whole text fits.

## Data and permissions

`SPENTDAY_DB` selects another database on both platforms, including release builds. Without it, macOS uses `~/Library/Application Support/SpentDay/spentday.db`; Linux uses `$XDG_DATA_HOME/spentday/spentday.db`, with `~/.local/share` as the fallback. The default data folder is created and reset to mode `0700` on startup. Failure to make it private is a startup error. An override leaves its parent folder alone. The core keeps database files and sidecars at `0600` because they contain the sync code.

`apps/desktop/macos/bundle.sh --dev` builds `macos/build/dev/Spent Day.app` with bundle id `day.spent.SpentDay.dev` and the separate `SpentDay Dev` data folder. `--demo` is a runtime flag; `SPENTDAY_DEMO` is no longer used.

macOS reads the front app through `NSWorkspace`, idle time through `CGEventSource`, media assertions through IOKit and the front window number through `CGWindowListCopyWindowInfo`. These need no Accessibility access. AppleScript reads the front browser window's private flag, id and active tab URL. Automation permission is requested once per scripted browser. Window and page titles are never read by these scripts. The core immediately reduces a URL to its domain and never stores or syncs a full address.

Chrome, Brave, Helium, Edge, Vivaldi and Opera use the front window's incognito flag. Arc uses its private flag. Chrome was tested live; Arc and the other forks still need live checks. Safari cannot identify private windows, so Safari sites stay off by default. Enabling Record Safari sites in Settings > General also records sites in private Safari windows. Firefox uses the [extension](../../extension/README.md), which reports no page for private windows. Other unscripted browsers use the extension or record app time without a site.

The extension listener binds only `127.0.0.1:7853`. It receives origins, never paths or titles. A private or non-web tab sends no page and ends the prior site's time. Another local program can send reports too; see [Privacy](../../docs/PRIVACY.md).

## Linux sessions

| Session | App in front | Idle source |
|---|---|---|
| X11 | `_NET_ACTIVE_WINDOW`, `WM_CLASS`, or process name | XScreenSaver |
| Sway | `swaymsg -t get_tree` | `ext-idle-notify-v1`, else ScreenSaver D-Bus |
| Hyprland | `hyprctl activewindow -j` | Same as Sway |
| GNOME Wayland | Installed GNOME Shell extension, app id only | Mutter IdleMonitor |
| KDE Wayland | Installed KWin script | ScreenSaver D-Bus |

Sway and Hyprland replies include window titles. The collector parses them in memory, takes the app id and drops the rest. No title reaches the core.

After 180 seconds without input, away time starts at the last input. Idle inhibitors or media in the focused app keep the user present. Checks run every 5 seconds on a worker, with a 2-second deadline per round. A positive check lasts 30 seconds; failed or missing checks do not renew it. Music in another app does not count. Sleep and lock close the open span. Without an idle source, time away from the keyboard counts as focused-app time and the app logs a warning. `SPENTDAY_AWAY_AFTER_SECS` shortens the threshold for tests; `SPENTDAY_COLLECTOR` selects `x11`, `sway`, `hyprland`, `gnome` or `kde`.

The systemd user service needs `DISPLAY` or `WAYLAND_DISPLAY` in the user manager environment and `graphical-session.target`. GNOME and KDE provide these. Sway and Hyprland need configuration and do not run autostart entries themselves. A compositor startup line is often simpler:

```text
# Sway
exec ~/.local/bin/spentday --background
# Hyprland
exec-once = ~/.local/bin/spentday --background
```

For a Sway systemd setup, import the compositor environment with `exec systemctl --user import-environment WAYLAND_DISPLAY SWAYSOCK XDG_CURRENT_DESKTOP` and configure the graphical session target as described by the compositor.

The GNOME helper answers only Spent Day's focus-bus owner. KWin reports are accepted only from KWin's connection. Unsandboxed programs running as the same user can read the database or load KWin scripts; these checks do not isolate them. Sway and Hyprland command timeouts stop their process group, but wrappers that create another session can leave child processes running. Presence sampling can miss a few seconds around media changes. Wayland idle watches the first seat, so input on another seat may count as away.

Real GNOME, KDE and Hyprland sessions remain manual checks. Tests use GNOME and KWin stand-ins and a recorded Hyprland reply. See [Testing](../../docs/TESTING.md).

## Signing

`bundle.sh` uses `SPENTDAY_SIGN_IDENTITY`, otherwise a valid local `Spent Day Dev` identity, otherwise ad-hoc signing. The bundle uses the hardened runtime with the Apple-events entitlement. Stable identity, bundle id and executable name preserve Automation grants across updates; ad-hoc rebuilds ask again.

`make-dev-cert.sh` creates and trusts a self-signed identity and lets `/usr/bin/codesign` use its key without a password prompt. Any program running as the same user can then sign with that identity and reuse its Automation grants. Use it only on a Mac that is not shared. Remove it with `security delete-identity -c "Spent Day Dev" -t`; Keychain Access labels its key "Imported Private Key". An Apple Development identity offers a prompt per signing operation only if its key's access controls require one; Always Allow removes that distinction.

## Release signing

Run `scripts/make-release-cert.sh` once. It writes a ten-year `Spent Day Release` identity and password into a temporary directory and prints the `gh secret set` commands for `MACOS_SIGN_P12` and `MACOS_SIGN_P12_PASSWORD`. It changes no keychain or GitHub settings itself. Keep a private backup, set the repository secrets and remove the temporary files. Reuse the same identity for later releases.

Tag and manual release builds import the identity into a temporary keychain, trust it for code signing and remove it after the build. Without secrets they sign ad hoc and warn. Pull request builds always sign ad hoc and never import secrets. The app is not notarized. After every download, including updates, allow the blocked launch in System Settings > Privacy & Security > Open Anyway.

A manual Release workflow run uploads a DMG and Arch package without publishing. Download the DMG and check its mounted app:

```sh
codesign --verify --strict '/Volumes/Spent Day/Spent Day.app'
codesign -dv --verbose=4 '/Volumes/Spent Day/Spent Day.app' 2>&1
```

Verification must pass. A release signed with the configured identity must not report `Signature=adhoc`; a pull request artifact does.

## Checks and screenshots

From the repository root:

```sh
(cd apps/desktop && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked && cargo build --locked)
apps/desktop/macos/test.sh
apps/desktop/macos/verify.sh
apps/desktop/linux/verify.sh
apps/desktop/snapshot.sh            # current OS
apps/desktop/snapshot.sh --linux    # Linux in Docker
```

Snapshots render every dashboard, settings, panel and fatal scene offscreen into `screenshots/macos/` or `screenshots/linux/`, plus light and 720x560 day views. Headless tooling always uses the demo store, even without `--demo`, and never opens the user's database. See [Testing](../../docs/TESTING.md) for commands and manual gaps.
