#!/usr/bin/env bash
# Checks publish.sh when a move fails, as on a full disk, and when it is killed between moves.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
work=""
trap 'rm -rf "$work"' EXIT
work="$(mktemp -d)"
failures=0
check() {
  if eval "$1"; then echo "ok    $2"; else echo "FAIL  $2"; failures=$((failures + 1)); fi
}

mkdir -p "$work/shims"
cat > "$work/shims/mv" <<'SHIM'
#!/bin/bash
if [ -n "${FAIL_MV_TO:-}" ] && [ "${@: -1}" = "$FAIL_MV_TO" ] && [ ! -e "$FAIL_MV_TO.failed" ]; then
  touch "$FAIL_MV_TO.failed"
  exit 1
fi
if [ -n "${FAIL_MV_FROM:-}" ] && [ "${@: -2:1}" = "$FAIL_MV_FROM" ]; then
  exit 1
fi
if [ -n "${TERM_ON_MV_TO:-}" ] && [ "${@: -1}" = "$TERM_ON_MV_TO" ]; then
  kill -TERM "$PPID"
  sleep 1
  exit 1
fi
exec /bin/mv "$@"
SHIM
chmod +x "$work/shims/mv"

bundle() { mkdir -p "$work/$1"; echo "$2" > "$work/$1/version"; }
leftovers() { ls -A "$work/out" | grep -v -e '^App$' -e '\.failed$' || true; }

mkdir -p "$work/out"
bundle out/App old
bundle new new
status=0
PATH="$work/shims:$PATH" FAIL_MV_TO="$work/out/App" "$here/publish.sh" "$work/new" "$work/out/App" 2>/dev/null || status=$?
check '[ "$status" != 0 ]' "a failed publish exits nonzero"
check '[ "$(cat "$work/out/App/version" 2>/dev/null)" = old ]' "a failed publish puts the old bundle back"
check '[ -z "$(leftovers)" ]' "a failed publish leaves no backup behind (found: $(leftovers))"

"$here/publish.sh" "$work/new" "$work/out/App"
check '[ "$(cat "$work/out/App/version")" = new ]' "a publish replaces the old bundle"
check '[ -z "$(leftovers)" ]' "a publish deletes the backup after it succeeds (found: $(leftovers))"

rm -rf "$work/out/App"
bundle new2 first
"$here/publish.sh" "$work/new2" "$work/out/App"
check '[ "$(cat "$work/out/App/version")" = first ]' "a first publish needs no old bundle"

rm -rf "$work/out" "$work/new"; mkdir -p "$work/out"
bundle out/App old
bundle new new
status=0
PATH="$work/shims:$PATH" TERM_ON_MV_TO="$work/out/App" "$here/publish.sh" "$work/new" "$work/out/App" 2>/dev/null || status=$?
check '[ "$status" != 0 ]' "a publish killed between moves exits nonzero"
check '[ "$(cat "$work/out/App/version" 2>/dev/null)" = old ]' "a publish killed between moves puts the old bundle back"
check '[ -z "$(leftovers)" ]' "a publish killed between moves leaves no backup behind (found: $(leftovers))"

rm -rf "$work/out" "$work/new"; mkdir -p "$work/out"
bundle out/App old
bundle new new
status=0
PATH="$work/shims:$PATH" FAIL_MV_FROM="$work/out/App" "$here/publish.sh" "$work/new" "$work/out/App" 2>/dev/null || status=$?
check '[ "$status" != 0 ]' "a failed backup move exits nonzero"
check '[ "$(cat "$work/out/App/version" 2>/dev/null)" = old ]' "a failed backup move leaves the old bundle in place"
check '[ -z "$(leftovers)" ]' "a failed backup move leaves no empty backup dir (found: $(leftovers))"

exit $((failures > 0))
