#!/usr/bin/env bash
# Cross-device sync with the three real apps. The macOS app, the Linux app in the dev container
# (apps/linux/Dockerfile.dev) and the Android app on a headless emulator join one local
# spentday-server, record activity, and sync. Each database must end up with all three devices,
# and each device's total for today must be the same in all three.
#
#   scripts/e2e-cross-device.sh
#
# Needs the Mac's screen unlocked, and some input on it in the last 2 hours: the macOS app counts
# no time without either. Opens Finder, TextEdit and the e2e app's dashboard for a few seconds. The e2e app is a copy with
# the bundle id day.spent.SpentDay.e2e and its own database, so the installed Spent Day, its database
# and its defaults are never touched. Screenshots go to /tmp/spentday-e2e/shots. Exits nonzero on
# the first failed check, and stops every process, container and emulator it started.
#
# It builds all three apps. The macOS and Android apps build on this Mac, so it needs every tool
# the README lists for them. The Linux app builds in the dev container, so it also needs Docker.
# On top of those it needs jq and the Android emulator. JAVA_HOME and ANDROID_HOME
# default to the Homebrew paths below, so set them if yours are elsewhere. It boots the AVD named
# in SPENTDAY_AVD, spentday-test by default, and nothing creates it. To make one with the
# Android 16 image this script was run with:
#
#   sdkmanager "emulator" "platform-tools" "system-images;android-36;google_apis_playstore;arm64-v8a"
#   avdmanager create avd -n spentday-test -d pixel_7 -k "system-images;android-36;google_apis_playstore;arm64-v8a"

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
work=/tmp/spentday-e2e
shots="$work/shots"
export JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@21}"
export ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
adb_bin="$ANDROID_HOME/platform-tools/adb"
serial=emulator-5580
avd="${SPENTDAY_AVD:-spentday-test}"
package=day.spent.android
container=spentday-e2e-linux
mac_bundle_id=day.spent.SpentDay.e2e
mac_app="$work/Spent Day.app"
mac_db="$work/mac.db"
linux_db=/tmp/e2e/linux.db
cli="$root/target/debug/spentday-cli"

server_pid=""
mac_pid=""
holder_pid=""
emulator_pid=""
container_started=""
density_changed=""
adb_was_running=1
textedit_was_running=1

step() { printf '\n== %s\n' "$*"; }
pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'; }

adb() { "$adb_bin" -s "$serial" "$@"; }
linux() { docker exec "$container" /src/scripts/e2e-linux.sh "$@"; }

stop_pid() {
  kill -TERM "$1" 2>/dev/null || return 0
  for _ in $(seq 40); do kill -0 "$1" 2>/dev/null || return 0; sleep 0.25; done
  return 1
}

stop_mac() {
  [ -n "$mac_pid" ] || return 0
  stop_pid "$mac_pid" || fail "the macOS app ignored SIGTERM"
  wait "$mac_pid" 2>/dev/null || true
  mac_pid=""
}

cleanup() {
  local status=$?
  set +e
  if [ -n "$mac_pid" ]; then stop_pid "$mac_pid" || kill -9 "$mac_pid"; fi
  [ -z "$holder_pid" ] || stop_pid "$holder_pid" || kill -9 "$holder_pid"
  if [ "$textedit_was_running" = 0 ] && pgrep -x TextEdit >/dev/null; then
    osascript -e 'tell application id "com.apple.TextEdit" to quit saving no' >/dev/null
  fi
  [ -z "$server_pid" ] || stop_pid "$server_pid" || kill -9 "$server_pid"
  [ -z "$container_started" ] || docker rm -f "$container" >/dev/null 2>&1
  if [ -n "$emulator_pid" ]; then
    [ -z "$density_changed" ] || adb shell wm density reset
    adb emu kill >/dev/null 2>&1
    for _ in $(seq 60); do kill -0 "$emulator_pid" 2>/dev/null || break; sleep 0.5; done
    kill -0 "$emulator_pid" 2>/dev/null && kill -9 "$emulator_pid"
    for _ in $(seq 20); do "$adb_bin" devices | grep -q "^$serial" || break; sleep 0.5; done
  fi
  [ "$adb_was_running" = 1 ] || "$adb_bin" kill-server >/dev/null 2>&1
  if [ "$status" = 0 ]; then echo "PASS: all checks"; else echo "FAIL: exit $status, see $work" >&2; fi
}
trap cleanup EXIT

step "leftovers from an interrupted run"
pkill -f "^$mac_app/Contents/MacOS/SpentDay" || true
docker rm -f "$container" >/dev/null 2>&1 || true
pgrep -f 'adb.*fork-server' >/dev/null || adb_was_running=0
# adb lists a stopped emulator as offline for a while. Only a live one holds the port.
if "$adb_bin" devices | grep -q "^$serial[[:space:]]*device"; then
  [ "$(adb emu avd name 2>/dev/null | head -1 | tr -d '\r')" = "$avd" ] || fail "$serial is another emulator. Stop it first."
  adb emu kill >/dev/null
  for _ in $(seq 60); do "$adb_bin" devices | grep -q "^$serial" || break; sleep 0.5; done
fi
rm -rf "$work"
mkdir -p "$shots" "$work/android"
pgrep -x TextEdit >/dev/null || textedit_was_running=0

step "boot the $avd emulator, headless"
"$ANDROID_HOME/emulator/emulator" -avd "$avd" -port "${serial#emulator-}" -no-window -no-audio \
  -no-boot-anim -no-snapshot-save -gpu swiftshader_indirect >"$work/emulator.log" 2>&1 &
emulator_pid=$!

step "spentday-server and a sync code"
(cd "$root" && cargo build -q -p spentday-server -p spentday-cli)
port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("", 0)); print(s.getsockname()[1])')
"$root/target/debug/spentday-server" --listen "0.0.0.0:$port" --db "$work/server.db" >"$work/server.log" 2>&1 &
server_pid=$!
for _ in $(seq 50); do curl -sf "http://127.0.0.1:$port/health" >/dev/null && break; sleep 0.1; done
curl -sf "http://127.0.0.1:$port/health" >/dev/null || fail "spentday-server did not answer on port $port"
code=$("$cli" new-code)
[[ "$code" =~ ^([0-9A-Z]{4}-){13}[0-9A-Z]{4}$ ]] || fail "new-code printed '$code'"
pass "spentday-server on 0.0.0.0:$port, sync code ${code:0:4}-..."

# Batches the server holds from device $1. The server sees device ids, never content.
batches_from() { sqlite3 "$work/server.db" "select count(*) from batches where device_id = '$1'"; }

step "build the macOS app"
# The dev build reads SPENTDAY_DB. A normal build ignores it.
"$root/apps/macos/build.sh" --dev >"$work/build-macos.log" 2>&1 || { tail -20 "$work/build-macos.log"; fail "apps/macos/build.sh --dev"; }
cp -R "$root/apps/macos/build/dev/Spent Day.app" "$mac_app"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $mac_bundle_id" "$mac_app/Contents/Info.plist"
codesign --force --sign - --timestamp=none --options runtime \
  --entitlements "$root/apps/macos/SpentDay.entitlements" "$mac_app" 2>/dev/null
[ "$(defaults read "$mac_app/Contents/Info" CFBundleIdentifier)" = "$mac_bundle_id" ] || fail "bundle id not changed"
# The e2e copy's own defaults domain. Skipping onboarding keeps it from opening a window at launch.
defaults write "$mac_bundle_id" onboardingDone -bool true
helper="$work/e2e-mac-helper"
xcrun swiftc -O "$root/scripts/e2e-mac-helper.swift" -o "$helper" 2>>"$work/build-macos.log" || fail "e2e-mac-helper.swift"
pass "built $mac_app as $mac_bundle_id"

step "build the Android app"
(cd "$root/apps/android" && ./gradlew --no-daemon -q -Pkotlin.compiler.execution.strategy=in-process assembleDebug) \
  >"$work/build-android.log" 2>&1 || { tail -30 "$work/build-android.log"; fail "assembleDebug"; }
apk="$root/apps/android/app/build/outputs/apk/debug/app-debug.apk"
[ -f "$apk" ] || fail "no $apk"
pass "built $(basename "$apk")"

step "build the Linux app in the dev container"
docker build -q -t spentday-linux-dev -f "$root/apps/linux/Dockerfile.dev" "$root/apps/linux" >/dev/null
docker run -d --init --name "$container" --hostname spentday-linux --add-host=host.docker.internal:host-gateway \
  -v "$root":/src:ro -v spentday-cargo:/opt/cargo/registry -v spentday-e2e-target:/target -e CARGO_TARGET_DIR=/target \
  spentday-linux-dev sleep infinity >/dev/null
container_started=1
linux build >"$work/build-linux.log" 2>&1 || { tail -30 "$work/build-linux.log"; fail "Linux build"; }
pass "built spentday and spentday-cli for Linux"

step "Linux: run the app under Xvfb and join sync"
linux desktop
linux start --fresh
linux stop
# The core takes plain http:// only for loopback and private addresses, so the
# container uses the private address host.docker.internal stands for.
host_ip=$(docker exec "$container" getent ahostsv4 host.docker.internal | awk 'NR == 1 { print $1 }')
[ -n "$host_ip" ] || fail "host.docker.internal has no IPv4 address in the container"
docker exec "$container" sh -c "for _ in \$(seq 30); do curl -sf -m 2 http://$host_ip:$port/health >/dev/null && exit 0; sleep 1; done; exit 1" \
  || fail "the container cannot reach spentday-server at $host_ip:$port"
# The sync code goes on stdin, never on a command line.
printf '%s\n' "$code" | docker exec -i "$container" /src/scripts/e2e-linux.sh cli sync --db "$linux_db" --server "http://$host_ip:$port" >/dev/null
linux cli devices --json --db "$linux_db" | jq -e 'map(.platform) == ["linux"]' >/dev/null || fail "the Linux database knows other devices already"
pass "the Linux database joined sync"

step "macOS: run the e2e app, join sync, switch apps"
start_mac() {
  env -u SPENTDAY_DEMO SPENTDAY_DB="$mac_db" "$mac_app/Contents/MacOS/SpentDay" "$@" >>"$work/mac.log" 2>&1 &
  mac_pid=$!
}
mac_devices() { "$cli" devices --json --db "$mac_db" 2>/dev/null; }
locked() { ioreg -n Root -d1 | grep -q '"IOConsoleLocked" = Yes'; }
for i in $(seq 600); do locked || break; [ "$i" = 1 ] && echo "waiting up to 10 min for this Mac's screen to be unlocked"; sleep 1; done
locked && fail "this Mac's screen is locked, so no app can come to the front and its app records nothing"
idle_s=$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ && !seen { print int($NF / 1e9); seen = 1 }')
[ "$idle_s" -lt 6600 ] || fail "no input on this Mac for $idle_s s. Its app counts the user away after 2 h even with the display held awake."
"$helper" hold-front &
holder_pid=$!
# Finder goes in front first, so the e2e app never scripts the browser the user had in front.
osascript -e 'tell application id "com.apple.finder" to activate'
start_mac
for _ in $(seq 40); do mac_devices >/dev/null && break; sleep 0.25; done
mac_devices >/dev/null || fail "the macOS app did not create $mac_db"
stop_mac
printf '%s\n' "$code" | "$cli" sync --db "$mac_db" --server "http://127.0.0.1:$port" >/dev/null
mac_id=$(mac_devices | jq -r '.[0].id')
pushed=$(batches_from "$mac_id")
start_mac
# The app syncs 10 s after launch.
for _ in $(seq 40); do [ "$(batches_from "$mac_id")" -gt "$pushed" ] && break; sleep 0.5; done
[ "$(batches_from "$mac_id")" -gt "$pushed" ] || fail "the macOS app did not push within 20 s of launch"
pass "the macOS app pushed on its own"
osascript -e 'tell application id "com.apple.TextEdit" to activate'
sleep 6
osascript -e 'tell application id "com.apple.finder" to activate'
sleep 6
stop_mac
stop_pid "$holder_pid"
holder_pid=""
if [ "$textedit_was_running" = 0 ]; then osascript -e 'tell application id "com.apple.TextEdit" to quit saving no'; fi
"$cli" sync --db "$mac_db" >/dev/null
recorded=$("$cli" spans --json --limit 1000 --db "$mac_db" | jq -r '[.[] | select(.end_ms > .start_ms) | .app_id] | unique | join(" ")')
[[ " $recorded " == *" com.apple.finder "* && " $recorded " == *" com.apple.TextEdit "* ]] \
  || fail "the macOS app recorded [$recorded], not Finder and TextEdit"
pass "the macOS app recorded Finder and TextEdit, quit on SIGTERM, and synced"

step "Linux: switch xterms"
linux start
# The app syncs at launch. Its database joined before the Mac had pushed anything.
linux_has_mac() { linux cli spans --json --limit 1000 --db "$linux_db" | jq -e 'any(.app_id == "com.apple.TextEdit")' >/dev/null; }
for _ in $(seq 40); do linux_has_mac && break; sleep 0.5; done
linux_has_mac || fail "the Linux app did not pull the Mac's TextEdit time within 20 s of launch"
pass "the Linux app pulled the Mac's activity on its own"
linux focus
linux stop
linux cli sync --db "$linux_db" >/dev/null
recorded=$(linux cli spans --json --limit 1000 --db "$linux_db" | jq -r '[.[] | select(.end_ms > .start_ms) | .app_id] | unique | join(" ")')
[[ " $recorded " == *" SpentDayA "* && " $recorded " == *" SpentDayB "* ]] || fail "the Linux app recorded [$recorded], not both xterms"
pass "the Linux app recorded both xterms, quit on SIGTERM, and synced"

step "Android: install on the emulator, join sync, open apps"
"$adb_bin" -s "$serial" wait-for-device
for _ in $(seq 180); do [ "$(adb shell getprop sys.boot_completed | tr -d '\r')" = 1 ] && break; sleep 1; done
[ "$(adb shell getprop sys.boot_completed | tr -d '\r')" = 1 ] || fail "$avd did not boot"
adb uninstall "$package" >/dev/null 2>&1 || true
adb shell "settings put global device_name 'Spent Day E2E Pixel'"
adb install "$apk" >/dev/null
adb shell appops set "$package" GET_USAGE_STATS allow
adb shell am start -W -n "$package/.MainActivity" >/dev/null
sleep 3
for app in com.android.settings com.google.android.deskclock; do
  activity=$(adb shell cmd package resolve-activity --brief -a android.intent.action.MAIN -c android.intent.category.LAUNCHER "$app" | tail -1 | tr -d '\r')
  [[ "$activity" == "$app/"* ]] || fail "$app has no launcher activity on $avd"
  adb shell am start -W -n "$activity" >/dev/null
  sleep 6
done
adb shell input keyevent KEYCODE_HOME
sleep 1
# Joins and syncs through the debug build's DebugSyncReceiver, the sync screen's code path.
android_sync() {
  local out
  out=$(adb shell am broadcast -n "$package/.DebugSyncReceiver" "$@" | tr -d '\r')
  [[ "$out" == *'result=-1'* ]] || fail "Android sync: $out"
  sed -n 's/.*data="\(.*\)"/\1/p' <<<"$out"
}
report=$(android_sync --es server "http://10.0.2.2:$port" --es code "$code")
echo "Android: $report"
[[ "$report" =~ received\ ([0-9]+) ]] && [ "${BASH_REMATCH[1]}" -gt 0 ] || fail "the Android app received nothing: $report"
pass "the Android app pushed its activity and pulled the others"

step "converge: every device syncs again"
report=$(android_sync)
echo "Android: $report"
adb shell am force-stop "$package"
"$cli" sync --db "$mac_db" >/dev/null
linux cli sync --db "$linux_db" >/dev/null
pass "macOS and Linux pulled after Android's last push"

step "assert: three devices with the same totals in every database"
read -r from to < <(python3 -c '
import datetime as d
t = d.datetime.now().astimezone().replace(hour=0, minute=0, second=0, microsecond=0)
print(int(t.timestamp() * 1000), int((t + d.timedelta(days=1)).replace(tzinfo=None).astimezone().timestamp() * 1000))')
for f in spentday.db spentday.db-wal spentday.db-shm; do
  adb exec-out run-as "$package" sh -c "cat files/$f 2>/dev/null" >"$work/android/$f"
done
[ -s "$work/android/spentday.db" ] || fail "could not copy the Android database"
android_db="$work/android/spentday.db"

# Prints `devices --json` and today's `summary --json` of one database as one JSON object.
# Today is the host's local day, passed as --from/--to so the UTC container uses the same range.
read_db() {
  case "$1" in
    macos) jq -n --argjson d "$("$cli" devices --json --db "$mac_db")" \
             --argjson s "$("$cli" summary --json --from "$from" --to "$to" --db "$mac_db")" '{devices: $d, summary: $s}' ;;
    linux) jq -n --argjson d "$(linux cli devices --json --db "$linux_db")" \
             --argjson s "$(linux cli summary --json --from "$from" --to "$to" --db "$linux_db")" '{devices: $d, summary: $s}' ;;
    android) jq -n --argjson d "$("$cli" devices --json --db "$android_db")" \
             --argjson s "$("$cli" summary --json --from "$from" --to "$to" --db "$android_db")" '{devices: $d, summary: $s}' ;;
  esac
}
for db in macos linux android; do
  read_db "$db" >"$work/$db.json"
  platforms=$(jq -r '[.devices[].platform] | sort | join(",")' "$work/$db.json")
  [ "$platforms" = "android,linux,macos" ] || fail "$db database has devices [$platforms], not android,linux,macos"
  [ "$(jq -r '.devices[0].platform' "$work/$db.json")" = "$db" ] || fail "$db database's own device is not listed first"
  empty=$(jq -r '[.summary.by_device[] | select(.total_ms > 0) | .device.platform] as $p
    | ["android", "linux", "macos"] - $p | join(",")' "$work/$db.json")
  [ -z "$empty" ] || fail "$db database has no time today for [$empty]"
  pass "$db database: 3 devices (android, linux, macos), each with time today"
done

jq -s -r '
  (map(.summary.by_device | map({(.device.id): .total_ms}) | add)) as [$mac, $linux, $android]
  | (.[0].devices | sort_by(.platform)) as $devices
  | ["device", "platform", "in mac.db", "in linux.db", "in android.db"],
    ($devices[] | [.name, .platform, $mac[.id], $linux[.id], $android[.id]])
  | @tsv' "$work/macos.json" "$work/linux.json" "$work/android.json" | column -t -s $'\t' | tee "$work/totals.txt"
mismatch=$(jq -s -r '
  map(.summary.by_device | map({(.device.id): .total_ms}) | add) as $dbs
  | [$dbs[0] | keys[] as $id | select(($dbs | map(.[$id]) | unique | length) > 1) | $id] | join(" ")' \
  "$work/macos.json" "$work/linux.json" "$work/android.json")
[ -z "$mismatch" ] || fail "per-device totals differ across databases for $mismatch"
pass "each device's total is identical in all three databases"

step "dashboard screenshots"
names=$(jq -r '[.devices[].name] | join(", ")' "$work/macos.json")
echo "device names to look for: $names"

start_mac --dashboard
for _ in $(seq 40); do window=$("$helper" window-id "$mac_pid"); [ -n "$window" ] && break; sleep 0.25; done
[ -n "$window" ] || fail "the macOS dashboard window did not open"
sleep 4
screencapture -x -o -l "$window" "$shots/macos-dashboard.png"
stop_mac
[ -s "$shots/macos-dashboard.png" ] || fail "no macOS screenshot"
pass "macOS dashboard: $shots/macos-dashboard.png"

linux shot /tmp/e2e/linux-dashboard.png
docker cp "$container:/tmp/e2e/linux-dashboard.png" "$shots/linux-dashboard.png" >/dev/null
[ -s "$shots/linux-dashboard.png" ] || fail "no Linux screenshot"
pass "Linux dashboard: $shots/linux-dashboard.png"

# At the default density the device chips run off the screen after the second one.
adb shell wm density 240
density_changed=1
adb shell am start -W -n "$package/.MainActivity" >/dev/null
sleep 4
adb exec-out screencap -p >"$shots/android-dashboard.png"
adb shell am force-stop "$package"
adb shell wm density reset
density_changed=""
[ -s "$shots/android-dashboard.png" ] || fail "no Android screenshot"
pass "Android dashboard: $shots/android-dashboard.png"
