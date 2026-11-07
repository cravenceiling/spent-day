#!/usr/bin/env bash
# Checks that make-dev-cert.sh removes what it imported when it fails or is interrupted, keeps it
# when it finishes, and stops when a "Spent Day Dev" that isn't valid is already there.
# `security` is a shim that writes down its calls, so no keychain and no trust setting is touched.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0
mkdir -p "$work/shims" "$work/home"

# STOP_AT names the call that fails (exit 1) or sends a signal to make-dev-cert.sh (HUP, INT,
# TERM). find-identity lists the identity once add-trusted-cert has run, and EXISTING=invalid
# lists one that `-v` leaves out from the start.
cat > "$work/shims/security" <<'SH'
#!/bin/sh
echo "$*" >> "$CALLS"
[ "$1" = "${STOP_AT%%:*}" ] && case "${STOP_AT#*:}" in
  fail) exit 1 ;;
  *) kill "-${STOP_AT#*:}" "$PPID"; exit 0 ;;
esac
if [ "$1" = find-identity ]; then
  if grep -q '^add-trusted-cert' "$CALLS" || { [ "${EXISTING:-}" = invalid ] && [ "$2" != -v ]; }; then
    echo '  1) ABCD "Spent Day Dev"'
  fi
fi
exit 0
SH
chmod +x "$work/shims/security"

# run <what> <STOP_AT> <exit status> <whether it removes the identity: yes or no> [EXISTING]
run() {
  : > "$work/calls"
  status=0
  CALLS="$work/calls" STOP_AT="$2" EXISTING="${5:-}" HOME="$work/home" PATH="$work/shims:$PATH" \
    "$here/make-dev-cert.sh" >"$work/out" 2>&1 || status=$?
  removed=no
  grep -q '^delete-identity -Z [0-9A-F]\{40\} -t ' "$work/calls" && removed=yes
  if [ "$status" = "$3" ] && [ "$removed" = "$4" ]; then
    echo "ok    make-dev-cert.sh $1 (exit $status)"
  else
    echo "FAIL  make-dev-cert.sh $1 (exit $status, want $3; removed: $removed, want $4)"
    sed 's/^/      /' "$work/calls" "$work/out"
    failures=$((failures + 1))
  fi
}

# expect <what> <grep pattern> on the calls of the last run
expect() {
  if grep -q -- "$2" "$work/calls"; then
    echo "ok    make-dev-cert.sh $1"
  else
    echo "FAIL  make-dev-cert.sh $1"
    failures=$((failures + 1))
  fi
}

run "keeps the identity when it finishes" "none:" 0 no
expect "imports the certificate" '^import .*/cert\.pem -k '
expect "imports the key for codesign only" '^import .*/key\.pem -k .* -x -T /usr/bin/codesign$'
expect "trusts the certificate for code signing" '^add-trusted-cert -r trustRoot -p codeSign '
[ "$(grep -c '^find-identity -v -p codesigning' "$work/calls")" = 2 ] \
  && echo "ok    make-dev-cert.sh checks the new identity is valid at the end" \
  || { echo "FAIL  make-dev-cert.sh checks the new identity is valid at the end"; failures=$((failures + 1)); }
run "removes the identity when trusting it fails" "add-trusted-cert:fail" 1 yes
run "removes the identity on SIGTERM" "add-trusted-cert:TERM" 143 yes
run "removes the identity on SIGHUP" "add-trusted-cert:HUP" 129 yes
run "removes the identity on SIGINT" "add-trusted-cert:INT" 130 yes
run "stops when one that isn't valid is there" "none:" 1 no invalid
if ! grep -q '^import' "$work/calls" && grep -q 'security delete-identity -c "Spent Day Dev" -t' "$work/out"; then
  echo "ok    make-dev-cert.sh imports nothing then, and says how to remove the old one"
else
  echo "FAIL  make-dev-cert.sh imports nothing then, and says how to remove the old one"
  failures=$((failures + 1))
fi

exit "$((failures > 0))"
