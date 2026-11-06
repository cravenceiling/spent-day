#!/usr/bin/env bash
# End to end: builds Spent Day with --dev, runs it on a scratch database, switches between Finder
# and TextEdit, quits (which flushes), then checks the spans the core recorded
# against the front app this script saw. Other apps may come to the front during
# the run; their time just has to be recorded too. Exits nonzero on any mismatch.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
app="$here/build/dev/Spent Day.app"
tolerance_ms=1500

work=""
watcher=""
spentday=""
# Installed before anything it cleans up exists, so an early failure leaves nothing behind.
cleanup() {
  [ -z "$watcher" ] || kill "$watcher" 2>/dev/null || true
  [ -z "$spentday" ] || kill -9 "$spentday" 2>/dev/null || true
  [ -z "$work" ] || rm -rf "$work"
}
trap cleanup EXIT
work="$(mktemp -d)"
# The database lives and dies with the temp dir. SPENTDAY_DB from the caller is never read,
# so a database the caller points at is never touched.
db="$work/spentday.db"

now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'; }
fail() { echo "FAIL: $*" >&2; exit 1; }

"$here/bundle.sh" --dev

idle_s=$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ && !seen { print int($NF / 1e9); seen = 1 }')
if [ "$idle_s" -gt 150 ]; then
  fail "no input for ${idle_s}s, so Spent Day would count this run as away. Move the mouse and run again."
fi

textedit_was_running=0
pgrep -x TextEdit >/dev/null && textedit_was_running=1

# The dev build has its own defaults domain. Marking onboarding done keeps it from opening a window.
defaults write day.spent.SpentDay.dev onboardingDone -bool true
# SPENTDAY_DEMO from the caller would swap in demo data and make the check meaningless.
env -u SPENTDAY_DEMO SPENTDAY_DB="$db" "$app/Contents/MacOS/SpentDay" --background &
spentday=$!
# Writes "<ms> <bundle id>" whenever the front app changes.
(
  last=""
  while :; do
    front="$(lsappinfo info -only bundleid "$(lsappinfo front)" | sed -n 's/.*="\(.*\)"/\1/p')"
    if [ "$front" != "$last" ]; then echo "$(now_ms) $front"; last="$front"; fi
    sleep 0.1
  done
) > "$work/front.log" &
watcher=$!
sleep 3

started=$(now_ms)
osascript -e 'tell application id "com.apple.finder" to activate'
sleep 12
osascript -e 'tell application id "com.apple.TextEdit" to activate'
sleep 12
osascript -e 'tell application id "com.apple.finder" to activate'
sleep 8
quit=$(now_ms)
kill "$watcher"
watcher=""

kill -TERM "$spentday"
for _ in $(seq 20); do kill -0 "$spentday" 2>/dev/null || break; sleep 0.5; done
kill -0 "$spentday" 2>/dev/null && fail "Spent Day ignored SIGTERM"
spentday=""
echo "Spent Day quit on SIGTERM"
if [ "$textedit_was_running" = 0 ]; then
  osascript -e 'tell application id "com.apple.TextEdit" to quit saving no'
fi

cd "$root"
spans="$(cargo run -q -p spentday-cli -- spans --json --limit 1000 --db "$db")"

# The front app's visits between the first switch and quit, from the watcher's log.
seen="$(awk -v from="$started" -v to="$quit" '
  { at[NR] = $1; id[NR] = $2 }
  END {
    for (i = 1; i <= NR; i++) {
      start = at[i] < from ? from : at[i]
      end = i < NR ? at[i + 1] : to
      if (end > to) end = to
      if (end - start >= 1000) printf "{\"app\":\"%s\",\"start\":%d,\"end\":%d}\n", id[i], start, end
    }
  }' "$work/front.log" | jq -s .)"

# Recorded visits: consecutive spans of one app (a site change splits them) merged.
recorded="$(jq --argjson from "$started" --argjson to "$quit" 'sort_by(.start_ms)
  | map(select(.end_ms > .start_ms))
  | reduce .[] as $s ([];
      if length > 0 and .[-1].app == $s.app_id and $s.start_ms - .[-1].end <= 1000
      then .[-1].end = $s.end_ms
      else . + [{app: $s.app_id, start: $s.start_ms, end: $s.end_ms}] end)
  | map(select(.end > $from and .start < $to)
        | .start = ([.start, $from] | max) | .end = ([.end, $to] | min)
        | select(.end - .start >= 1000))' <<<"$spans")"

echo "== front app (seen by this script)"
jq -r '.[] | "\(.start)  \(.end - .start)ms  \(.app)"' <<<"$seen"
echo "== recorded"
jq -r '.[] | "\(.start)  \(.end - .start)ms  \(.app)"' <<<"$recorded"

overlaps="$(jq '[sort_by(.start_ms) | map(select(.end_ms > .start_ms)) as $s
  | range(1; $s | length) | select($s[. - 1].end_ms > $s[.].start_ms)] | length' <<<"$spans")"
[ "$overlaps" = 0 ] || fail "$overlaps spans overlap the span before them"

# Every visit seen has a recorded visit of the same app within the tolerance at both ends, and the reverse.
unmatched() {
  jq -r --argjson other "$2" --argjson tol "$tolerance_ms" '.[] as $v
    | select([$other[] | select(.app == $v.app
        and ((.start - $v.start) | fabs) <= $tol and ((.end - $v.end) | fabs) <= $tol)] | length == 0)
    | "\($v.app) \($v.start)..\($v.end) (\($v.end - $v.start)ms)"' <<<"$1"
}
missing="$(unmatched "$seen" "$recorded")"
[ -z "$missing" ] || fail "in front but not recorded within ${tolerance_ms}ms:
$missing"
extra="$(unmatched "$recorded" "$seen")"
[ -z "$extra" ] || fail "recorded but not in front within ${tolerance_ms}ms:
$extra"

for target in com.apple.finder com.apple.TextEdit; do
  held="$(jq --arg app "$target" '[.[] | select(.app == $app) | .end - .start] | add // 0' <<<"$seen")"
  [ "$held" -ge 1000 ] || fail "$target never stayed in front for 1s, so there is nothing to check. Run again."
done

echo "PASS: every visit, Finder and TextEdit included, recorded within ${tolerance_ms}ms of when it was in front, no overlaps"
