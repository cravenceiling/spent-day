#!/bin/sh
# The Linux half of scripts/e2e-cross-device.sh. Runs inside the spentday-linux-dev container
# (apps/desktop/linux/Dockerfile.dev), one step per `docker exec`:
#
#   e2e-linux.sh build                   builds the app and spentday-cli
#   e2e-linux.sh desktop                 starts Xvfb, openbox and a session bus
#   e2e-linux.sh start [--fresh]         runs `spentday --background` on $DB until it tracks
#   e2e-linux.sh stop                    stops it with SIGTERM, which flushes the open span
#   e2e-linux.sh focus                   switches focus between two xterms
#   e2e-linux.sh cli ARGS...             runs spentday-cli
#   e2e-linux.sh shot PNG                screenshots the dashboard
set -eu
state=/tmp/e2e
db=$state/linux.db
target="${CARGO_TARGET_DIR:-/target}"
spentday="$target/debug/spentday"
cli="$target/debug/spentday-cli"
mkdir -p "$state"
[ -f "$state/env" ] && . "$state/env"
export LANG=C.UTF-8
export VK_ICD_FILENAMES="${VK_ICD_FILENAMES:-/usr/share/vulkan/icd.d/lvp_icd.json}"

fail() { echo "FAIL: linux: $*" >&2; exit 1; }

# Runs a command detached from this `docker exec`, so it outlives it.
daemon() { name=$1; shift; setsid "$@" >"$state/$name.log" 2>&1 < /dev/null & echo $! > "$state/$name.pid"; }

alive() { [ -f "$state/$1.pid" ] && kill -0 "$(cat "$state/$1.pid")" 2>/dev/null; }

stop() {
  alive "$1" || return 0
  kill -TERM "$(cat "$state/$1.pid")"
  for _ in $(seq 50); do alive "$1" || { rm -f "$state/$1.pid"; return 0; }; sleep 0.2; done
  fail "$1 ignored SIGTERM"
}

case "$1" in
build)
  (cd /src/apps/desktop && cargo build --quiet --locked)
  (cd /src && cargo build --quiet --locked -p spentday-cli)
  ls -l "$spentday" "$cli"
  ;;
desktop)
  daemon xvfb Xvfb :99 -screen 0 1100x1000x24
  echo "export DISPLAY=:99" > "$state/env"
  export DISPLAY=:99
  for _ in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
  xdpyinfo >/dev/null 2>&1 || fail "Xvfb did not start"
  daemon openbox openbox
  echo "export DBUS_SESSION_BUS_ADDRESS='$(dbus-daemon --session --fork --print-address)'" >> "$state/env"
  ;;
start)
  [ "${2:-}" = --fresh ] && rm -f "$db" "$db-wal" "$db-shm"
  alive spentday && fail "spentday is already running"
  SPENTDAY_DB="$db" daemon spentday "$spentday" --background
  for _ in $(seq 100); do grep -q "tracking with" "$state/spentday.log" && break; sleep 0.2; done
  grep -q "tracking with" "$state/spentday.log" || { cat "$state/spentday.log"; fail "spentday did not start tracking"; }
  ;;
stop)
  stop spentday
  ;;
focus)
  daemon xterm-a xterm -class SpentDayA -T "spentday window A" -e sleep 600
  daemon xterm-b xterm -class SpentDayB -T "spentday window B" -e sleep 600
  xdotool search --sync --class SpentDayA >/dev/null
  xdotool search --sync --class SpentDayB >/dev/null
  for i in 1 2 3; do
    # Pointer motion is the input that keeps the idle monitor from counting the user as away.
    xdotool mousemove $((100 + i)) 100
    xdotool search --class SpentDayA windowactivate --sync; sleep 5
    xdotool search --class SpentDayB windowactivate --sync; sleep 5
  done
  stop xterm-a
  stop xterm-b
  ;;
cli)
  shift
  "$cli" "$@"
  ;;
shot)
  SPENTDAY_DB="$db" daemon window "$spentday"
  xdotool search --sync --onlyvisible --all --pid "$(cat "$state/window.pid")" --name '^Spent Day$' >/dev/null
  sleep 3
  import -window root "$2"
  xdotool key ctrl+w
  stop window
  ;;
*)
  fail "unknown step $1"
  ;;
esac
