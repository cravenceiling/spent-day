# spentday-cli

Reads and writes a Spent Day database from the command line. Verification scripts use it to fake devices and to check what the dashboard would show. `scripts/cli-e2e.sh` runs two fake devices through a local `spentday-server` and prints the merged summary.

```sh
spentday-cli summary --db PATH [--today | --from MS --to MS] [--bucket-ms 3600000] [--device ID] [--json]
spentday-cli spans   --db PATH [--limit 50] [--json]      # raw rows, newest first
spentday-cli devices --db PATH [--json]
spentday-cli sync    --db PATH [--server URL]             # with --server, stores it and a sync code, then syncs
spentday-cli new-code

# Fake a device. --name and --platform create the database (and rename the device);
# later calls can omit them. --at defaults to now.
spentday-cli simulate --db PATH [--name NAME --platform macos|linux|android|ios|windows] EVENT
  heartbeat --at MS --app-id ID [--app-name N] [--url U]
  away      --at MS
  flush     --at MS
  session   --start MS --end MS [--every 5000] --app-id ID [--app-name N] [--url U]
  interval  --start MS --end MS --app-id ID [--app-name N] [--url U]
  hint      --at MS --browser B --url U
```

`sync --server` reads the sync code from `SPENTDAY_SYNC_CODE`, or from the first line of stdin when that is unset, so the code never shows up in the process list:

```sh
printf '%s\n' "$code" | spentday-cli sync --db PATH --server https://sync.example.com
```

`--url` takes a full URL, but only its domain is stored, as in the apps.

`summary` with neither `--today` nor `--from/--to` covers today, local midnight to midnight. Every read command opens the database without changing its device name or platform, so it is safe to point at a running app's database.
