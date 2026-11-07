#!/usr/bin/env bash
# publish.sh <bundle> <dest>: moves <bundle> to <dest>, replacing what is there.
# The old <dest> waits in a backup beside it and comes back if the publish fails or
# is interrupted, so a failed publish never leaves no app. The backup is deleted only
# after success.
set -euo pipefail

bundle="$1"
dest="$2"
name="$(basename "$dest")"
backup=""
published=""

finish() {
  local status=$?
  trap - EXIT HUP INT TERM
  if [ -z "$published" ] && [ -n "$backup" ]; then
    if [ -e "$backup/$name" ] && ! /bin/mv "$backup/$name" "$dest"; then
      echo "Couldn't put the new $name in place or the old one back. The old one is in $backup." >&2
      exit 1
    fi
    rmdir "$backup" 2>/dev/null || true
    echo "Couldn't put the new $name in place. $dest is unchanged." >&2
  elif [ -n "$backup" ]; then
    rm -rf "$backup"
  fi
  exit "$status"
}
trap finish EXIT
trap 'exit 143' HUP INT TERM

if [ -e "$dest" ]; then
  backup="$(mktemp -d "$(dirname "$dest")/.$name.old.XXXXXX")"
  mv "$dest" "$backup/$name"
fi
mv "$bundle" "$dest"
published=1
