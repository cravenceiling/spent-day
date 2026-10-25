#!/usr/bin/env bash
# Checks that verify.sh removes its temp dir when it fails early, that it never touches
# a database the caller points SPENTDAY_DB at, and how it launches the app. The app is a
# stub, and every command that could show a window or write settings is a shim.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'xargs rm -rf < "$work/made" 2>/dev/null; rm -rf "$work"' EXIT
failures=0

mkdir -p "$work/macos" "$work/shims"
touch "$work/made"
cp "$here/verify.sh" "$work/macos/verify.sh"
# Records every path mktemp makes, so the check can see whether verify.sh removed it.
printf '#!/bin/sh\npath="$(/usr/bin/mktemp "$@")" || exit\necho "$path" >> "%s"\necho "$path"\n' "$work/made" > "$work/shims/mktemp"
# Reports an hour without input, so verify.sh stops right after the build, unless IDLE_NS says otherwise.
printf '#!/bin/sh\necho "    \\"HIDIdleTime\\" = ${IDLE_NS:-3600000000000}"\n' > "$work/shims/ioreg"
# Records `defaults` calls instead of writing settings.
printf '#!/bin/sh\necho "$*" >> "%s"\n' "$work/defaults.log" > "$work/shims/defaults"
# No app is activated, no time passes, and the recorded spans are empty.
for tool in osascript sleep lsappinfo; do printf '#!/bin/sh\nexit 0\n' > "$work/shims/$tool"; done
printf '#!/bin/sh\necho "[]"\n' > "$work/shims/cargo"
chmod +x "$work/shims"/*
for f in real.db real.db-wal real.db-shm; do echo keep > "$work/$f"; done

# run <what> <build.sh body>
run() {
  printf '#!/bin/sh\n%s\n' "$2" > "$work/macos/build.sh"
  chmod +x "$work/macos/build.sh"
  : > "$work/made"
  status=0
  PATH="$work/shims:$PATH" SPENTDAY_DB="$work/real.db" "$work/macos/verify.sh" >/dev/null 2>&1 || status=$?
  made="$(wc -l < "$work/made" | tr -d ' ')"
  left="$(while read -r path; do [ ! -e "$path" ] || echo "$path"; done < "$work/made")"
  kept="$(cat "$work/real.db" "$work/real.db-wal" "$work/real.db-shm" 2>/dev/null | tr -d '\n' || true)"
  if [ "$status" != 0 ] && [ "$made" -gt 0 ] && [ -z "$left" ]; then
    echo "ok    verify.sh removes its temp dir when $1"
  else
    echo "FAIL  verify.sh removes its temp dir when $1 (exit $status, made $made, left: $left)"
    failures=$((failures + 1))
  fi
  if [ "$kept" = keepkeepkeep ]; then
    echo "ok    verify.sh leaves the SPENTDAY_DB database alone when $1"
  else
    echo "FAIL  verify.sh leaves the SPENTDAY_DB database alone when $1"
    failures=$((failures + 1))
  fi
}

run "the build fails" 'echo "build failed on purpose" >&2; exit 1'
run "it stops after the build" 'exit 0'

# The launch: the stub app writes down the environment it got and exits.
mkdir -p "$work/macos/build/dev/Spent Day.app/Contents/MacOS"
printf '#!/bin/sh\nenv > "%s"\n' "$work/app.env" > "$work/macos/build/dev/Spent Day.app/Contents/MacOS/SpentDay"
chmod +x "$work/macos/build/dev/Spent Day.app/Contents/MacOS/SpentDay"
printf '#!/bin/sh\nexit 0\n' > "$work/macos/build.sh"
: > "$work/made"
PATH="$work/shims:$PATH" IDLE_NS=0 SPENTDAY_DEMO=1 SPENTDAY_DB="$work/real.db" "$work/macos/verify.sh" >/dev/null 2>&1 || true
app_db="$(sed -n 's/^SPENTDAY_DB=//p' "$work/app.env" 2>/dev/null || true)"
case "$app_db" in
  "$(head -1 "$work/made")"/*) echo "ok    verify.sh runs the app on a database in its own temp dir" ;;
  *) echo "FAIL  verify.sh runs the app on a database in its own temp dir (got '$app_db')"; failures=$((failures + 1)) ;;
esac
if [ -f "$work/app.env" ] && ! grep -q '^SPENTDAY_DEMO=' "$work/app.env"; then
  echo "ok    verify.sh doesn't pass the caller's SPENTDAY_DEMO to the app"
else
  echo "FAIL  verify.sh doesn't pass the caller's SPENTDAY_DEMO to the app"
  failures=$((failures + 1))
fi
if [ "$(cat "$work/defaults.log" 2>/dev/null)" = 'write day.spent.SpentDay.dev onboardingDone -bool true' ]; then
  echo "ok    verify.sh skips onboarding for the dev bundle id only"
else
  echo "FAIL  verify.sh skips onboarding for the dev bundle id only (defaults calls: $(cat "$work/defaults.log" 2>/dev/null))"
  failures=$((failures + 1))
fi
exit $((failures > 0))
