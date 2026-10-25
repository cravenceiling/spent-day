# spentday-core

The Rust core every Spent Day app links. The public contract is `src/lib.rs`, and the behavior it implements is in `docs/DESIGN.md`. Swift and Kotlin reach it through UniFFI. The Linux app and `spentday-cli` use it as a Rust crate.

## Modules

| File | Owns |
|---|---|
| `lib.rs` | Public types and the `SpentDay` object. Locks the store and calls into the modules below. |
| `apps.rs` | Canonical app table: platform ids to one key and display name, plus which apps are browsers. |
| `store.rs` | SQLite in WAL mode. Schema migrations keyed on `PRAGMA user_version`. Keeps the database private to its owner. |
| `tracker.rs` | Heartbeats and intervals become spans. |
| `web.rs` | Web hints, `domain_at`, interval splitting, domain normalization. |
| `summary.rs` | Totals, buckets, per-device, per-app and per-site rows. |
| `sync.rs` | Sync code, HKDF keys, XChaCha20-Poly1305 batches, HTTP client. |
| `listener.rs` | `POST /v1/web` and `/v1/health` on 127.0.0.1 for the browser extension. The extension's popup calls `/v1/health` with POST, and GET works too. |

## Building

Run every command from the repository root.

```sh
cargo test --workspace

# UniFFI bindings
cargo build -p spentday-core --release
cargo run -p spentday-core --features bindgen --bin uniffi-bindgen -- \
  generate --library target/release/libspentday_core.dylib --language swift --out-dir /tmp/spentday-bind-swift
cargo run -p spentday-core --features bindgen --bin uniffi-bindgen -- \
  generate --library target/release/libspentday_core.dylib --language kotlin --out-dir /tmp/spentday-bind-kotlin

# Android (.so per ABI under target/<triple>/release/)
ANDROID_NDK_HOME=/opt/homebrew/share/android-commandlinetools/ndk/28.2.13676358 \
  cargo ndk -t arm64-v8a -t x86_64 build -p spentday-core --release

# macOS (.a and .dylib)
cargo build -p spentday-core --target aarch64-apple-darwin --release

# Linux, from macOS. The separate target dir keeps Linux artifacts out of target/debug.
docker run --rm -v "$PWD":/src -w /src -e CARGO_TARGET_DIR=/src/target/docker-linux \
  rust:latest cargo build -p spentday-core -p spentday-server -p spentday-cli
```

The Kotlin generator tries to run `ktlint` and prints a warning when it is missing. The file is still written.

## Behavior apps rely on

- **Heartbeats.** A heartbeat extends the open span when its `(app_id, domain)` matches and it arrives at most 30 s after the previous one. A different key extends the old span up to this heartbeat, then opens a new one. A longer gap leaves the old span ending at its last heartbeat. `heartbeat(at, None)` and `flush(at)` extend the open span to `at` (if within 30 s) and close it. `away_since(last_input, now)` does the same but ends the span at `max(its start, last_input)` at the latest, cutting time already recorded after the last input with a new `rev`. The open span id is stored in the database, so a restarted app keeps extending it. A heartbeat stamped before the latest end of any span this device stored (the clock stepped back, or a callback arrived late) is dropped when its key matches the open span. Otherwise it opens its span at that latest end, so heartbeat spans from one device never overlap, even across `flush` and `away_since`.
- **What is stored.** App ids, app names, domains, times and the device name. `Activity` and `WebPage` have no title, and the schema has no column for a URL or a title. A `WebPage` URL is cut to its domain where it enters the core (`web::normalize_domain`), and the URL goes no further: not into a span, a hint, a summary or a sync batch. A host longer than 253 bytes, the DNS limit, counts as no site. App ids and app names are cut to 512 bytes, and the device name passed to `open` to 256 bytes, on a char boundary.
- **File permissions.** On Unix the database holds the sync code in plain text, so `open` creates missing directories `0700` and sets the database and its `-wal`, `-shm` and `-journal` files to `0600`, existing ones too, whatever the umask. Only the directories that call creates become `0700`, and directories that already exist are left alone. A database path that is a symlink or a `file:` URI is refused, because SQLite would put the `-wal` and `-shm` files where they are not protected, and so is a sidecar that is a symlink. The core creates the file with `O_EXCL`, which never follows a symlink, and changes each existing file through an open handle only when it is the inode `lstat` saw. A database file or sidecar the core can't read, such as one set to `0200`, makes `open` fail with an error that names the file and says to run `chmod 600` on it. The core doesn't change it by path, because `chmod` would follow a symlink swapped in after the check. The resolved path stays a byte path all the way to SQLite, so a folder whose name is not UTF-8 still opens the file that was checked. SQLite opens the path with its folders resolved and `SQLITE_OPEN_NOFOLLOW`, so a symlink swapped in after the check is not followed either. If any of this fails, `open` fails. `open_existing` sets the files to `0600` and leaves the directory alone.
- **Early databases.** Development builds stored URLs and titles, and there is no migration from them. `open` refuses such a database with an `OldDatabase` error, which carries the path and says to delete it. Every other open failure stays a `Storage` error, so apps offer "start over" only for this one.
- **Web hints.** Send `browser` as a canonical key (`safari`, `chrome`, `chromium`, `firefox`, `brave`, `edge`, `arc`, `opera`, `vivaldi`, `zen`, `samsung-internet`, `duckduckgo`) or any platform id in `apps.rs`, such as `com.android.chrome`. A hint only attributes time to the browser it names. On the heartbeat path, a hint whose browser is not in the table also attributes to whichever browser is focused. The extension sends `chromium-family` for a Chromium fork it cannot identify, and that key must stay out of the table. The interval path never uses those. Non-http(s) URLs are dropped, and only the domain of the rest is stored. `web_cleared(at, browser)` records a no-page hint (new tab, `chrome://`, a PDF, a private window): a heartbeat or interval piece whose latest hint is one gets no site. Hints stamped more than a day ahead of the system clock are dropped. Hints are kept for 7 days, counted back from the newest hint or from now, whichever is earlier, and pruned on open and on every new hint.
- **Heartbeat attribution.** A browser heartbeat with `web: None` takes the latest hint for that browser (or for a browser not in the table) at or before it, if that hint is at most 60 s old.
- **Intervals.** `record_interval` splits a browser interval at every hint for that browser inside it that changes the domain. Neighbouring pieces on the same domain, or both with no page, are one span. The first piece takes the latest hint for that browser at or before the interval start, if it is at most 30 minutes old. Each piece's id is a hash of `(device, app_id, piece start)`. Re-recording the same interval changes nothing, and a longer `end_ms` for the same start replaces the row. `end_ms <= start_ms` retracts the interval with that `(app_id, start_ms)`: each of its pieces becomes zero-length with a new `rev`. With no such interval it does nothing. When a re-recorded interval splits differently (a hint was pruned, or UsageStats reports a shorter end), each piece the new split lacks becomes a zero-length span with a new `rev`, so every synced device drops that time too. Summaries skip zero-length spans.
- **Summary.** Spans are clipped to `[start_ms, end_ms)`. `buckets` has `ceil((end - start) / bucket_ms)` slots, and is empty when `bucket_ms <= 0`. More than 10_000 slots fails with `InvalidArgument`. A range with `end_ms <= start_ms` gives an empty summary. `by_device` lists only devices with time in the range. Ties in every sorted list break by name.
- **Sync codes.** 56 Crockford base32 symbols in 14 groups of 4: 256 secret bits plus a 24-bit checksum, so a typo fails with `InvalidSyncCode` (about one in 16.7 million gets through). Input is case-insensitive and ignores spaces and dashes. `sync_config()` returns the canonical form. `set_sync_config` takes `https://` for any server, and plain `http://` only for loopback, `localhost` and private IP literals (10/8, 172.16/12, 192.168/16, fc00::/7). Anything else, `.local` names included, fails with `InvalidArgument`. It stores the URL as it was checked, and checks a stored URL again every time it syncs: plain `http://localhost` becomes `http://127.0.0.1`, so no name lookup decides where plain http goes. A server that listens only on `[::1]` must be entered as `http://[::1]:<port>`. The client follows no redirects. Changing the server or the code resets the push and pull positions in the same transaction, so the next sync pushes everything again. A sync already running for the old config stops at its next write and writes nothing more. Concurrent `sync_now` calls on one `SpentDay` run one after another. `sync_now` pushes in batches of at most 2000 spans and about 3 MiB of plaintext, then pulls. It still pulls when the push fails, and then returns the push error. A pulled batch that will not decrypt or parse is skipped for good and counted in `SyncReport::skipped_batches`. When the stored pull cursor is past the server's `head`, the pull reads the space again from cursor 0, at most once per sync (see "Cursor recovery" in `docs/DESIGN.md`). A device `platform` this build does not know reads as `linux`.
- **Listener.** `start_web_listener(port)` binds 127.0.0.1 only. It answers preflight and POST `/v1/web`, and preflight, GET and POST `/v1/health` (200 `{"ok":true,"device":"<name>"}`). Every request needs `Host` set to `127.0.0.1:<port>`, `localhost:<port>` or `[::1]:<port>`, and an `Origin` starting with `chrome-extension://` or `moz-extension://`. Anything else gets 403, a request without an `Origin` too. The body is parsed as JSON whatever its `Content-Type`, `"url"` may be a full URL or an origin, only its domain is kept, and `"url": null` is a no-page hint. Other fields, such as a `title`, are ignored. It serves one request per connection, each on its own thread, at most 16 at once. A connection past that gets 503 before anything of it is read. A client has 3 s to send its whole request, so one that stalls holds its slot for at most that long. Heads over 8 KiB, blank line included, get 431, bodies over 64 KiB 413, and chunked bodies 411. A head with a control character other than tab, a second `Host`, `Origin` or `Content-Length`, or a `Content-Length` that is not plain digits gets 400. A `Content-Length` is judged by its value, so leading zeros are fine.

## Rust-only API

`SpentDay::open_existing(path)` opens a database without changing its device name or platform. `SpentDay::recent_spans(limit)` returns raw `Span` rows. `spentday-cli` uses both. They are not exported through UniFFI.
