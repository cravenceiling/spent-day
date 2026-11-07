# Testing

## Automated checks

Run from the repository root:

```sh
cargo test --workspace
(cd apps/desktop && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked && cargo build --locked)
apps/desktop/macos/test.sh
apps/desktop/macos/verify.sh
apps/desktop/linux/verify.sh
node --test extension/test/*.test.mjs
(cd apps/android && ./gradlew testDebugUnitTest)
actionlint .github/workflows/release.yml
shellcheck packaging/linux/*.sh packaging/linux/AppRun
```

The desktop crate has its own workspace and lockfile. Root workspace tests cover the core, server and CLI. The macOS scripts cover collector tests, live media assertions, signing and focus-to-span checks. Linux verification builds its Docker image, checks X11 focus and idle, Chromium and the extension, helper stand-ins, single-instance activation, installer behavior and headless Sway.

The lifecycle test runs each quit path in a child process because winit permits one event loop per process. It checks `AppHandle::quit`, `Tx::quit`, SIGTERM with and without a window, and SIGINT. A missing display skips these cases; run them under a desktop or Xvfb to exercise the event loop. Every path must return so the app can stop tracking and flush after the loop.

## Linux checks in Docker

Until the full verification image is needed, this runs formatting, clippy, tests and a build with recent Mesa. The unit-specific target directory avoids contention with other workers.

```sh
docker run --rm \
  -v "$PWD:/src" \
  -v spentday-desktop-target:/target \
  -v spentday-desktop-cargo-registry:/usr/local/cargo/registry \
  -v spentday-desktop-cargo-git:/usr/local/cargo/git \
  -e CARGO_TARGET_DIR=/target/unit6 \
  -w /src/apps/desktop rust:1-trixie bash -c '
    apt-get update -qq &&
    apt-get install -y -qq --no-install-recommends libxkbcommon-dev libwayland-dev pkg-config mesa-vulkan-drivers libvulkan1 libxkbcommon-x11-0 libx11-6 libx11-xcb1 libxcursor1 libxrandr2 libxi6 xauth xvfb &&
    rustup component add rustfmt clippy &&
    cargo fmt --check &&
    cargo clippy --locked --all-targets -- -D warnings &&
    xvfb-run -a cargo test --locked &&
    cargo build --locked'
```

## Linux release packages

`packaging/linux/Dockerfile` uses the pinned Rust compiler on Ubuntu 24.04 for `.deb` builds and Ubuntu 22.04 for AppImage builds. `build-deb.sh` pins cargo-deb. `build-appimage.sh` pins appimagetool and its runtime by release and SHA-256. Neither build enables GCC LTO for the C dependencies.

Run the same scripts as CI from the repository root:

```sh
docker build --build-arg BASE=ubuntu:24.04@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852 -t spentday-deb -f packaging/linux/Dockerfile packaging/linux
docker build -t spentday-appimage -f packaging/linux/Dockerfile packaging/linux
docker volume create spentday-release-target
docker run --rm -v "$PWD:/src" -v spentday-release-target:/target \
  -e CARGO_HOME=/target/cargo -e CARGO_INSTALL_ROOT=/target/cargo-deb -e CARGO_TARGET_DIR=/target/deb \
  spentday-deb /src/packaging/linux/build-deb.sh
docker run --rm -v "$PWD:/src" -v spentday-release-target:/target \
  -e CARGO_HOME=/target/cargo -e CARGO_TARGET_DIR=/target/appimage \
  spentday-appimage /src/packaging/linux/build-appimage.sh
docker volume rm spentday-release-target
```

On an ARM host, pass `--platform linux/amd64` to both build and run. Headless renders also need a current emulator with AVX/F16C support. Colima's QEMU 7 reports `shaderFloat16=false` in Mesa and cannot validate Rune's half-float shader operations. A current [BuildKit QEMU helper](https://github.com/tonistiigi/binfmt#buildkit-target), mounted only in the test container with `QEMU_CPU=max`, keeps the VM's shared emulator registration unchanged. Use only this named target volume and remove it after verification. The artifacts land in `dist/`.

Inspect the Debian package with `dpkg-deb -I` and `dpkg-deb -c`. Install it with `apt install ./spentday_<version>-1_amd64.deb` in a fresh `debian:trixie` container. With `mesa-vulkan-drivers` and fonts installed, run `spentday --demo --snap /tmp/day.png --scene day` and inspect the PNG. No display is needed for this scene.

In fresh `ubuntu:24.04` and `fedora:latest` containers, install the host Vulkan/Mesa, Wayland, xkbcommon and X11 runtime libraries plus fonts. Run the AppImage with `--appimage-extract-and-run --demo --snap /tmp/day.png --scene day` and inspect each PNG. Check the extracted AppDir contains no shared libraries, and check the binary's required GLIBC symbols against the documented floor.

Run `--install-helpers` with a temporary `HOME` and custom `XDG_DATA_HOME` from GNOME and KDE sessions. Check the paths and contents against the source helper files. Automated tests use command stand-ins; enabling helpers in a real GNOME or KDE Wayland session remains a manual check.

## Headless scenes

```sh
apps/desktop/snapshot.sh
apps/desktop/snapshot.sh --linux
apps/desktop/target/debug/spentday --demo --list-windows
apps/desktop/target/debug/spentday --demo --list-scenes
apps/desktop/target/debug/spentday --demo --scene day --snap /tmp/day.png
apps/desktop/target/debug/spentday --demo --scene day --theme light --size 720x560 --snap /tmp/day-light.png
apps/desktop/target/debug/spentday --demo --scene settings-on --tree /tmp/settings.json
apps/desktop/target/debug/spentday --demo --window panel --scene panel --snap /tmp/panel.png
apps/desktop/target/debug/spentday --demo --window fatal --scene db-error --snap /tmp/fatal.png
```

`snapshot.sh` captures every registered scene in `apps/desktop/screenshots/macos/` or `linux/`. Settings scenes belong to `dash`; only the panel and fatal scenes need another `--window`. It also captures light and small day views. Inspect each image for clipping, hover placement, focus rings and readable labels. Per-OS screenshots account for font rendering differences; no pixel comparison is required.

Headless commands always use demo data and skip tracking, the listener, timers and single-instance claims. The demo day is 28 October 2026 with four earlier Wednesdays; other days are empty. Demo scenes expect 13h 50m of wall time, 14h 15m of device time and a 47-minute weekday comparison. Settings trees must hide the full sync code until revealed.

## Manual checks and coverage limits

Earlier platform builds were exercised on macOS 26, Docker X11 and headless Sway, a Debian 13 Xfce VM, and an Android 16 emulator. Those checks establish collector behavior; the Rune desktop port still needs the live checks below after integration.

- Close the dashboard and confirm CLI `spans` continues growing. Reopen it and check the selected day and drill survive.
- Quit with Cmd/Ctrl+Q, SIGTERM and SIGINT. Confirm the open span flushes. On macOS also quit through an Apple event and logout.
- Start with `--background` and confirm no dashboard appears. Force a bad database path and confirm the fatal window appears and quits with code 3. Start without a display on Linux and confirm a logged error and exit code 1.
- Set `SPENTDAY_DB` to a missing scratch path and run `--demo`; confirm no database appears. Check headless rendering does the same without `--demo`.
- Launch Linux a second time and confirm it activates the primary window with the launcher's token. On Wayland, close and reopen the dashboard. Check real GNOME, KDE and Hyprland focus, idle and helper installation.
- On macOS check menu panel placement, click-away dismissal, repeated-click toggle, Escape, dashboard activation and first-launch onboarding. Check Safari opt-in, lock and sleep, and live private-window behavior in Brave, Helium, Edge, Vivaldi, Opera and Arc.
- Check VoiceOver and Orca on the treemap, settings fields and fatal window. Check reduced motion and live theme changes.
- Run the Release workflow on a pull request and inspect the DMG, Arch package, `.deb` and AppImage. Verify macOS signing and Linux runtime dependencies.

GNOME and KDE tests use stand-ins. Hyprland uses a recorded reply. Real phones and Android 8 through 15 remain untested. `scripts/e2e-cross-device.sh` checks sync across the real apps and needs a manual run after the port's integration.
