#!/bin/sh
# Builds and checks the Linux app and the browser extension in the dev container.
#
#   apps/linux/verify.sh          everything that can run
#   apps/linux/verify.sh unit     unit tests and build only
#
# On the host it builds apps/linux/Dockerfile.dev and reruns itself inside.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

if [ ! -e /.spentday-dev ]; then
  docker build -q -t spentday-linux-dev -f "$here/Dockerfile.dev" "$here" >/dev/null
  exec docker run --rm --name "spentday-verify-$(date +%s)-$$" --hostname spentday-linux -v "$root":/src -v spentday-cargo:/opt/cargo/registry -v spentday-target:/target \
    -e CARGO_TARGET_DIR=/target ${SPENTDAY_DOCKER_ARGS:-} spentday-linux-dev /src/apps/linux/verify.sh "$@"
fi
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  exec dbus-run-session -- "$0" "$@"
fi

step() { printf '\n== %s\n' "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
target="${CARGO_TARGET_DIR:-$root/target}"
spentday="$target/debug/spentday"
cli="$target/debug/spentday-cli"
shots="$here/screenshots"
export GSK_RENDERER=cairo GTK_A11Y=none NO_AT_BRIDGE=1 LANG=C.UTF-8

step "unit tests"
(cd "$here" && cargo test --quiet) || fail "unit tests"
step "core privacy tests (APFS refuses the non-UTF-8 folder name one of them needs)"
(cd "$root" && cargo test --quiet -p spentday-core --test privacy) || fail "core privacy tests"
step "extension tests"
node --test "$root"/extension/test/*.test.mjs || fail "extension tests"
step "build"
(cd "$here" && cargo build --quiet)
(cd "$root" && cargo build --quiet -p spentday-cli)
ls -l "$spentday" "$cli"
[ "${1:-}" = unit ] && exit 0

Xvfb :99 -screen 0 1280x1000x24 >/dev/null 2>&1 &
xvfb_pid=$!
export DISPLAY=:99
sleep 1
openbox >/dev/null 2>&1 &
openbox_pid=$!
sleep 1
mkdir -p "$shots"

step "integration tests against the session bus and the X server"
# They share one session bus, so one test's players must not show up in another's checks.
(cd "$here" && GTK_A11Y=test cargo test --quiet -- --ignored --test-threads=1) || fail "integration tests"

# Starts `spentday --background` on database $1 and waits until it tracks.
start_spentday() {
  rm -f "$1" "$1"-wal "$1"-shm
  SPENTDAY_DB="$1" "$spentday" --background >"$1.log" 2>&1 &
  spentday_pid=$!
  for _ in $(seq 50); do grep -q "tracking with" "$1.log" 2>/dev/null && break; sleep 0.2; done
  cat "$1.log"
}

stop_spentday() {
  kill -TERM "$spentday_pid"
  wait "$spentday_pid" || true
}

# Prints the spans of database $1 as a table and checks them with the Python expression $2,
# which sees `spans` (oldest first), `ids` (app ids with repeats collapsed) and
# `sites(app_id, since_ms)` (that app's domains with repeats collapsed).
check_spans() {
  "$cli" spans --db "$1" --limit 200 --json > "$1.json"
  python3 - "$1.json" "$2" <<'PY'
import json, sys
spans = sorted(json.load(open(sys.argv[1])), key=lambda s: s["start_ms"])
for s in spans:
    print(f'{(s["end_ms"] - s["start_ms"]) / 1000:6.1f}s  {s["app_id"]:<16} {s["domain"] or ""}')
ids = [s["app_id"] for i, s in enumerate(spans) if i == 0 or spans[i - 1]["app_id"] != s["app_id"]]
def sites(app_id, since):
    domains = [s["domain"] for s in spans if s["app_id"].lower() == app_id and s["start_ms"] >= since]
    return [d for i, d in enumerate(domains) if i == 0 or domains[i - 1] != d]
ok = eval(sys.argv[2])
print("check:", sys.argv[2], "->", "ok" if ok else "FAILED")
sys.exit(0 if ok else 1)
PY
}

# Fails when database $1, its WAL or its shared memory file holds any of the strings $2...,
# compared without case. Spent Day stores app ids, domains and times, never a URL or a title.
not_on_disk() {
  db=$1; shift
  for text in "$@"; do
    if cat "$db" "$db-wal" "$db-shm" 2>/dev/null | grep -a -i -q -F "$text"; then
      fail "'$text' reached $db"
    fi
  done
}

step "demo treemap screenshots"
"$here/snapshot.sh"

step "a database that will not open shows a dialog, then Spent Day exits"
printf 'not a database' > /tmp/bad.db
SPENTDAY_DB=/tmp/bad.db "$spentday" --background >/tmp/bad.log 2>&1 &
bad_pid=$!
bad_window=$(timeout 10 xdotool search --sync --onlyvisible --pid "$bad_pid" | head -1) || fail "no dialog for a bad database"
sleep 1
xdotool windowactivate --sync "$bad_window" key Escape
status=0; wait "$bad_pid" || status=$?
cat /tmp/bad.log
[ "$status" = 3 ] || fail "exit status $status after a bad database"

step "X11: focus switches between two xterms"
start_spentday /tmp/e2e.db
xterm -class SpentDayA -T "spentday window A" -e sleep 600 &
xterm_a_pid=$!
xterm -class SpentDayB -T "spentday window B" -e sleep 600 &
xterm_b_pid=$!
xdotool search --sync --class SpentDayA >/dev/null
xdotool search --sync --class SpentDayB >/dev/null
for _ in 1 2 3; do
  xdotool search --class SpentDayA windowactivate --sync; sleep 6
  xdotool search --class SpentDayB windowactivate --sync; sleep 6
done
kill "$xterm_a_pid" "$xterm_b_pid"
wait "$xterm_a_pid" 2>/dev/null || true
wait "$xterm_b_pid" 2>/dev/null || true
sleep 2
check_spans /tmp/e2e.db \
  'ids[-6:] == ["SpentDayA", "SpentDayB"] * 3 and all(4.5 <= (s["end_ms"] - s["start_ms"]) / 1000 <= 8 for s in spans[-6:-1])' \
  || fail "X11 spans"
not_on_disk /tmp/e2e.db "spentday window"

mkdir -p /tmp/site
printf '<title>Alpha page</title><h1>Alpha</h1>' > /tmp/site/alpha.html
printf '<title>Beta page</title><h1>Beta</h1>' > /tmp/site/beta.html
printf '<title>Secret page</title><h1>Secret</h1>' > /tmp/site/secret.html
(cd /tmp/site && exec python3 -m http.server 8000 >/dev/null 2>&1) &
site_pid=$!

# Starts Chromium with the extension on the URL $2, with extra flags $1.
start_chromium() {
  chromium --no-sandbox --disable-gpu --no-first-run --no-default-browser-check --password-store=basic \
    --user-data-dir=/tmp/chromium-profile --disable-features=DisableLoadExtensionCommandLineSwitch \
    --load-extension="$root/extension" $1 "$2" >>/tmp/chromium.log 2>&1 &
  chromium_pid=$!
  xdotool search --sync --onlyvisible --class chromium windowactivate --sync >/dev/null 2>&1 || true
}

stop_chromium() {
  kill "$chromium_pid"; wait "$chromium_pid" 2>/dev/null || true
  sleep 1
}

step "extension: an incognito window records no site"
since=$(date +%s%3N)
start_chromium --incognito http://localhost:8000/secret.html
sleep 15
stop_chromium
check_spans /tmp/e2e.db \
  "{s['domain'] for s in spans if s['app_id'].lower() == 'chromium' and s['start_ms'] >= $since} == {None}" \
  || fail "incognito spans"
not_on_disk /tmp/e2e.db localhost secret

# The two pages are on two hosts, localhost and 127.0.0.1, so they are two sites. A site change
# alone does not change the focused window, so it shows at the next 5 s heartbeat.
step "extension: tabs reach the app as domains, and a new tab stops the site"
since=$(date +%s%3N)
start_chromium "" http://localhost:8000/alpha.html
sleep 15
newtab=$(date +%s%3N)
xdotool key ctrl+t
sleep 12
xdotool key ctrl+l; sleep 0.5
xdotool type --delay 20 'http://127.0.0.1:8000/beta.html'
xdotool key Return
sleep 15
stop_chromium
check_spans /tmp/e2e.db \
  "sites('chromium', $since)[-3:] == ['localhost', None, '127.0.0.1'] \
   and max(s['end_ms'] for s in spans if s['domain'] == 'localhost' and s['start_ms'] >= $since) <= $newtab + 6500 \
   and sum(s['end_ms'] - s['start_ms'] for s in spans if s['app_id'].lower() == 'chromium' and s['start_ms'] >= $newtab and s['domain'] is None) >= 9000" \
  || fail "extension spans"
not_on_disk /tmp/e2e.db alpha beta

step "extension: the popup's health check"
curl -sf -D /tmp/health.headers -H 'Origin: chrome-extension://spentdaytest' http://127.0.0.1:7853/v1/health >/tmp/health.json \
  || fail "health request"
cat /tmp/health.json; echo
[ "$(cat /tmp/health.json)" = "{\"ok\":true,\"device\":\"$(hostname)\"}" ] || fail "health body"
grep -qi '^access-control-allow-origin: chrome-extension://spentdaytest' /tmp/health.headers || fail "health CORS header"
[ "$(curl -s -o /dev/null -w '%{http_code}' -H 'Origin: https://example.com' http://127.0.0.1:7853/v1/health)" = 403 ] \
  || fail "health answered a web page"
[ "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:7853/v1/health)" = 403 ] \
  || fail "health answered a request with no Origin"
[ "$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: rebind.example:7853' -H 'Origin: chrome-extension://spentdaytest' http://127.0.0.1:7853/v1/health)" = 403 ] \
  || fail "health answered a request for another host"
# Chromium derives an unpacked extension's id from its path.
ext_id=$(python3 -c 'import hashlib, sys; print(hashlib.sha256(sys.argv[1].encode()).hexdigest()[:32].translate(str.maketrans("0123456789abcdef", "abcdefghijklmnop")))' "$root/extension")
# Chromium blocks extension pages opened from the command line, but not typed ones. --test-type
# hides the bar that warns about --no-sandbox, and the crop keeps the popup and the toolbar.
start_chromium --test-type about:blank
sleep 3
xdotool key ctrl+l; sleep 0.5
xdotool type --delay 20 "chrome-extension://$ext_id/popup.html"
xdotool key Return
sleep 3
import -window root -crop 1100x260+0+0 +repage "$shots/extension-popup.png"
stop_chromium

step "real dashboard smoke screenshot"
SPENTDAY_DB=/tmp/e2e.db "$spentday" >/tmp/real-window.log 2>&1 &
real_pid=$!
wait "$real_pid"
real_window=$(timeout 15 xdotool search --sync --onlyvisible --all --pid "$spentday_pid" --name '^Spent Day$' | head -1)
sleep 3
import -window "$real_window" "$shots/dashboard-real.png"
cat /tmp/real-window.log
stop_spentday

step "X11: idle past the threshold ends the span at the last input"
xdotool mousemove 10 10
export SPENTDAY_AWAY_AFTER_SECS=8
start_spentday /tmp/away.db
unset SPENTDAY_AWAY_AFTER_SECS
xterm -class SpentDayIdle -T "spentday idle" -e sleep 600 &
xterm_idle_pid=$!
xdotool search --sync --class SpentDayIdle windowactivate --sync >/dev/null
sleep 3
xdotool mousemove 300 300
input=$(date +%s%3N)
sleep 20
xdotool mousemove 320 320
back=$(date +%s%3N)
sleep 6
stop_spentday
kill "$xterm_idle_pid"
wait "$xterm_idle_pid" 2>/dev/null || true
check_spans /tmp/away.db \
  "(lambda idle: len(idle) >= 2 and abs(idle[0]['end_ms'] - $input) <= 1500 and $back <= idle[-1]['start_ms'] <= $back + 2000)([s for s in spans if s['app_id'] == 'SpentDayIdle' and s['end_ms'] > s['start_ms']])" \
  || fail "away spans"

step "a swaymsg that never answers is killed, and quitting still takes under 4 s"
mkdir -p /tmp/hang
printf '#!/bin/sh\nsleep 600\n' > /tmp/hang/swaymsg
chmod +x /tmp/hang/swaymsg
export SPENTDAY_COLLECTOR=sway PATH="/tmp/hang:$PATH"
start_spentday /tmp/hang.db
unset SPENTDAY_COLLECTOR
export PATH="${PATH#/tmp/hang:}"
sleep 4
grep -q "swaymsg did not answer within 1 s" /tmp/hang.db.log || fail "no swaymsg timeout"
quit_at=$(date +%s%3N)
stop_spentday
quit_ms=$(( $(date +%s%3N) - quit_at ))
echo "quit took $quit_ms ms"
[ "$quit_ms" -lt 4000 ] || fail "quitting took $quit_ms ms"

step "GNOME: the extension answers Spent Day and turns down other callers"
gjs -m "$here/tests/gnome-shell.js" "$here/gnome-extension/extension.js" org.gnome.Nautilus >/tmp/shell.log 2>&1 &
shell_pid=$!
for _ in $(seq 50); do grep -q ready /tmp/shell.log && break; sleep 0.2; done
cat /tmp/shell.log
ask_extension() {
  gdbus call --session --dest org.gnome.Shell --object-path /day/spent/SpentDay/Focus \
    --method day.spent.SpentDay.Focus.Get 2>&1
}
ask_extension | grep -q AccessDenied || fail "the extension answered a caller that is not Spent Day"
export SPENTDAY_COLLECTOR=gnome
start_spentday /tmp/gnome.db
tracking=$(date +%s%3N)
unset SPENTDAY_COLLECTOR
sleep 6
ask_extension | grep -q AccessDenied || fail "the extension answered another caller while Spent Day runs"
# 6 is DBUS_NAME_FLAG_REPLACE_EXISTING with DBUS_NAME_FLAG_DO_NOT_QUEUE, and 3 is
# DBUS_REQUEST_NAME_REPLY_EXISTS.
taken=$(gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
  --method org.freedesktop.DBus.RequestName day.spent.SpentDay.Focus 6)
[ "$taken" = "(uint32 3,)" ] || fail "another connection took the focus name from Spent Day: $taken"
sleep 6
stopping=$(date +%s%3N)
stop_spentday
# Exactly one span, which starts when tracking starts and ends when Spent Day stops.
check_spans /tmp/gnome.db \
  "len(spans) == 1 and spans[0]['app_id'] == 'org.gnome.Nautilus' \
   and $tracking - 1000 <= spans[0]['start_ms'] <= $tracking + 1500 \
   and $stopping - 100 <= spans[0]['end_ms'] <= $stopping + 3000" \
  || fail "GNOME spans"

step "GNOME: with the focus name held by another program, Spent Day says so and exits"
gjs -c 'const {Gio, GLib} = imports.gi;
  Gio.bus_own_name(Gio.BusType.SESSION, "day.spent.SpentDay.Focus", Gio.BusNameOwnerFlags.NONE, null, () => print("held"), null);
  new GLib.MainLoop(null, false).run();' >/tmp/holder.log 2>&1 &
holder_pid=$!
for _ in $(seq 50); do grep -q held /tmp/holder.log && break; sleep 0.2; done
SPENTDAY_COLLECTOR=gnome SPENTDAY_DB=/tmp/held.db "$spentday" --background >/tmp/held.log 2>&1 &
held_pid=$!
held_window=$(timeout 10 xdotool search --sync --onlyvisible --pid "$held_pid" | head -1) || fail "no dialog when the focus name is held"
sleep 1
xdotool windowactivate --sync "$held_window" key Escape
status=0; wait "$held_pid" || status=$?
cat /tmp/held.log
[ "$status" = 3 ] || fail "exit status $status with the focus name held"
grep -q "Another program holds the session bus name day.spent.SpentDay.Focus" /tmp/held.log || fail "no reason in the log"
kill "$holder_pid" "$shell_pid"

step "Sway: focus switches between two foot windows (headless)"
export XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p -m 700 "$XDG_RUNTIME_DIR"
printf 'xwayland disable\n' > /tmp/sway.conf
env -u DISPLAY WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
sway_pid=$!
for _ in $(seq 50); do ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock >/dev/null 2>&1 && break; sleep 0.2; done
SWAYSOCK="$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock)"
WAYLAND_DISPLAY="$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-? | head -1)")"
export SWAYSOCK WAYLAND_DISPLAY
unset DISPLAY
start_spentday /tmp/sway.db
swaymsg -q exec 'foot -a spentday.foot.a -T "sway window A"'
swaymsg -q exec 'foot -a spentday.foot.b -T "sway window B"'
sleep 2
for _ in 1 2 3; do
  swaymsg -q '[app_id="spentday.foot.a"] focus'; sleep 6
  swaymsg -q '[app_id="spentday.foot.b"] focus'; sleep 6
done
stop_spentday
check_spans /tmp/sway.db \
  'ids[-6:] == ["spentday.foot.a", "spentday.foot.b"] * 3' \
  || fail "Sway spans"
not_on_disk /tmp/sway.db "sway window"

step "Sway: ext-idle-notify-v1 reports idle and resumed input (headless, no input devices)"
export SPENTDAY_AWAY_AFTER_SECS=5
start_spentday /tmp/sway-idle.db
unset SPENTDAY_AWAY_AFTER_SECS
grep -q "no idle source" /tmp/sway-idle.db.log && fail "no idle source on Sway"
swaymsg -q '[app_id="spentday.foot.a"] focus'
sleep 12
swaymsg -q 'seat - cursor move 5 5'
resumed=$(date +%s%3N)
# The span may start up to 1.5 s after the input and must last 2 s. With no more input the user
# is away again 5 s after it, which would cut the span back, so a second move keeps them present.
sleep 2.5
swaymsg -q 'seat - cursor move 5 5'
sleep 2.5
stop_spentday
kill "$sway_pid"
check_spans /tmp/sway-idle.db \
  "sum(s['end_ms'] - s['start_ms'] for s in spans if s['start_ms'] < $resumed - 1000) == 0 \
   and any(abs(s['start_ms'] - $resumed) <= 1500 and s['end_ms'] - s['start_ms'] >= 2000 for s in spans)" \
  || fail "Sway idle spans"

step "install.sh: two --systemd runs with CARGO_TARGET_DIR, a fake cargo and systemctl"
rm -rf /tmp/install && mkdir -p /tmp/install/bin /tmp/install/target/release /tmp/install/tmp
printf '#!/bin/sh\necho "cargo $*" >> /tmp/install/calls\n' > /tmp/install/bin/cargo
printf '#!/bin/sh\necho "systemctl $*" >> /tmp/install/calls\n[ "$2" = show-environment ] && printf "%%s\\n" $FAKE_MANAGER_ENV\nexit 0\n' > /tmp/install/bin/systemctl
chmod +x /tmp/install/bin/*
cp "$spentday" /tmp/install/target/release/spentday
install_spentday() {
  env -u DISPLAY -u WAYLAND_DISPLAY -u SWAYSOCK -u HYPRLAND_INSTANCE_SIGNATURE -u XDG_CURRENT_DESKTOP PATH="/tmp/install/bin:$PATH" HOME=/tmp/install/home TMPDIR=/tmp/install/tmp \
    CARGO_TARGET_DIR=/tmp/install/target "$@" "$here/install.sh" "${install_mode:---systemd}" >/tmp/install/out 2>&1 \
    || { cat /tmp/install/out; fail "install.sh"; }
  cat /tmp/install/out
}
install_spentday XDG_CURRENT_DESKTOP=test
install_spentday XDG_CURRENT_DESKTOP=test
cat /tmp/install/calls
cmp /tmp/install/home/.local/bin/spentday "$spentday" || fail "install.sh did not install from CARGO_TARGET_DIR"
grep -qx 'ExecStart="/tmp/install/home/.local/bin/spentday" --background' /tmp/install/home/.config/systemd/user/spentday.service \
  || fail "ExecStart"
grep -qx 'RestartPreventExitStatus=3' /tmp/install/home/.config/systemd/user/spentday.service \
  || fail "the service restarts after a dialog the user closed"
[ "$(grep -cx 'systemctl --user restart spentday.service' /tmp/install/calls)" = 2 ] || fail "reruns restart the service"
grep -q -- '--now' /tmp/install/calls && fail "install.sh still uses enable --now"
[ -z "$(ls -A /tmp/install/tmp)" ] || fail "install.sh left temporary files"
install_spentday XDG_CURRENT_DESKTOP=sway WAYLAND_DISPLAY=wayland-1 SWAYSOCK=/run/user/1000/sway-ipc.sock \
  FAKE_MANAGER_ENV="HOME=/tmp/install/home DISPLAY=:0"
grep -qxF 'To fix it, add to your Sway config:  exec systemctl --user import-environment WAYLAND_DISPLAY SWAYSOCK XDG_CURRENT_DESKTOP, then log in again.' /tmp/install/out \
  || fail "no import-environment line for Sway"
install_spentday XDG_CURRENT_DESKTOP=Hyprland WAYLAND_DISPLAY=wayland-1 HYPRLAND_INSTANCE_SIGNATURE=abc \
  FAKE_MANAGER_ENV="WAYLAND_DISPLAY=wayland-1 XDG_CURRENT_DESKTOP=Hyprland"
grep -qxF 'To fix it, add to hyprland.conf:  exec-once = dbus-update-activation-environment --systemd HYPRLAND_INSTANCE_SIGNATURE, then log in again.' /tmp/install/out \
  || fail "no dbus-update-activation-environment line for Hyprland"

step "install.sh: a prefix with spaces loads in systemd and launches through GIO"
spaced='/tmp/install/my apps/spent day'
install_spentday XDG_CURRENT_DESKTOP=test PREFIX="$spaced"
unit=/tmp/install/home/.config/systemd/user/spentday.service
grep ExecStart "$unit"
mkdir -p -m 700 /tmp/install/runtime
verified=$(XDG_RUNTIME_DIR=/tmp/install/runtime systemd-analyze verify --user "$unit" 2>&1) || { echo "$verified"; fail "systemd-analyze verify"; }
[ -z "$verified" ] || { echo "$verified"; fail "systemd-analyze verify complained"; }
install_mode=--autostart install_spentday XDG_CURRENT_DESKTOP=test PREFIX="$spaced"
# The stub records the arguments GIO starts it with.
printf '#!/bin/sh\nprintf "%%s\\n" "$0" "$@" > /tmp/install/argv\n' > "$spaced/bin/spentday"
for entry in "$spaced/share/applications/day.spent.SpentDay.desktop" /tmp/install/home/.config/autostart/day.spent.SpentDay.desktop; do
  rm -f /tmp/install/argv
  gjs -c 'const {Gio} = imports.gi;
    const app = Gio.DesktopAppInfo.new_from_filename(ARGV[0]);
    if (!app) throw new Error(`GIO did not load ${ARGV[0]}`);
    app.launch([], null);' "$entry" || fail "GIO could not launch $entry"
  for _ in $(seq 25); do [ -s /tmp/install/argv ] && break; sleep 0.2; done
  cat /tmp/install/argv
  [ "$(head -1 /tmp/install/argv)" = "$spaced/bin/spentday" ] || fail "GIO started the wrong binary from $entry"
done
[ "$(sed -n 2p /tmp/install/argv)" = --background ] || fail "the autostart entry lost --background"

step "install.sh: refuses a prefix with characters entries and units cannot hold, or a relative one"
tab=$(printf '\t'); cr=$(printf '\r'); esc=$(printf '\033'); del=$(printf '\177')
# refuse_prefix <what> <prefix> <reason it gives>
refuse_prefix() {
  status=0
  env PATH="/tmp/install/bin:$PATH" HOME=/tmp/install/home PREFIX="$2" "$here/install.sh" --no-start >/tmp/install/out 2>&1 || status=$?
  cat /tmp/install/out
  [ "$status" = 2 ] || fail "install.sh took a prefix with $1, status $status"
  grep -q "$3" /tmp/install/out || fail "no reason for refusing the prefix with $1"
}
for bad in '%' '$' '"' "'" '\' '`' "$tab" "$cr" "$esc" "$del" "
"; do
  refuse_prefix "$(printf '%s' "$bad" | od -An -c | tr -d ' ')" "/tmp/install/bad${bad}prefix" "Set PREFIX to another folder"
done
[ -z "$(find /tmp/install -maxdepth 1 -name 'bad*')" ] || fail "install.sh wrote to a refused prefix"
refuse_prefix "a relative path" "relative/prefix" "PREFIX must be an absolute path"
[ ! -e "$here/relative" ] || fail "install.sh wrote to a relative prefix"

kill "$site_pid" "$openbox_pid" "$xvfb_pid" 2>/dev/null || true
step "all checks passed"
