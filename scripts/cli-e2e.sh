#!/usr/bin/env bash
# Two fake devices sync through a local spentday-server; prints one device's summary.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q -p spentday-server -p spentday-cli
bin=target/debug
work=$(mktemp -d)
port=${PORT:-18787}
"$bin/spentday-server" --listen "127.0.0.1:$port" --db "$work/server.db" 2>"$work/server.log" &
server_pid=$!
trap 'kill $server_pid; rm -rf "$work"' EXIT
until curl -sf "http://127.0.0.1:$port/health" >/dev/null; do sleep 0.1; done

cli() { "$bin/spentday-cli" "$@"; }
mac="$work/mac.db"
phone="$work/phone.db"
cli simulate --db "$mac" --name "MacBook" --platform macos \
  session --start 0 --end 600000 --app-id com.google.Chrome --app-name "Google Chrome" --url https://www.youtube.com/watch?v=1
cli simulate --db "$mac" session --start 600000 --end 900000 --app-id com.tinyspeck.slackmacgap --app-name Slack
cli simulate --db "$phone" --name "Pixel" --platform android \
  hint --at 1000000 --browser com.android.chrome --url github.com/rust-lang
cli simulate --db "$phone" interval --start 1000000 --end 1240000 --app-id com.android.chrome --app-name Chrome
cli simulate --db "$phone" interval --start 1240000 --end 1300000 --app-id com.Slack --app-name Slack

code=$(cli new-code)
printf '%s\n' "$code" | cli sync --db "$mac" --server "http://127.0.0.1:$port"
SPENTDAY_SYNC_CODE=$code cli sync --db "$phone" --server "http://127.0.0.1:$port"
cli sync --db "$mac"
cli devices --db "$mac"
cli summary --db "$mac" --from 0 --to 3600000 --bucket-ms 900000 --json
