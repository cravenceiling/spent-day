# Spent Day design

Spent Day records which app (and which website) is in front of you on every device you own, and syncs those records end-to-end encrypted so each device shows one dashboard for all of them.

## Layout

| Path | What it is |
|---|---|
| `core/` | Rust crate `spentday-core`. Storage, span building, web attribution, summaries, sync client, crypto. Exposed to Swift and Kotlin through UniFFI, linked directly by the Linux app. |
| `server/` | Rust crate `spentday-server`. Dumb append-only store of encrypted batches. Self-hostable binary. |
| `cli/` | Rust crate `spentday-cli`. Reads a Spent Day database and prints summaries as JSON. Used by every verification script. |
| `apps/macos/` | Swift menu bar app (SwiftUI dashboard). |
| `apps/linux/` | Rust GTK4 + libadwaita app. |
| `apps/android/` | Kotlin + Jetpack Compose app. |
| `extension/` | WebExtension (Chrome, Firefox) that reports the active tab to the desktop app on `127.0.0.1:7853`. |

## Data model

Every device writes only its own rows. That one rule removes sync conflicts: merging is a union keyed by `(device_id, span_id)`, and the owning device's highest `rev` wins.

- **Device.** `id` (UUID v4, generated once per install), `name`, `platform`.
- **Activity.** What a collector sees right now: an app (`app_id` such as `com.apple.Safari`, `org.mozilla.firefox`, `com.android.chrome`, and `app_name`) and an optional `web` part holding a `url`. `None` means the user is away (idle, locked, asleep).
- **Span.** A closed or still-growing interval `[start_ms, end_ms)` of one activity on one device. Columns: `device_id, id, start_ms, end_ms, app_id, app_name, domain, rev`.
- **Web hint.** `(at_ms, browser, domain)` reported by the extension or the Android accessibility service. Persisted so interval-based collectors can split browser time after the fact. A **no-page hint** (`web_cleared(at, browser)`, stored with an empty domain) says the browser shows no web page: a new tab, `chrome://`, a PDF, or a private window.

A platform may hand the core a full URL, but the core cuts it to its domain where it comes in and keeps nothing else of it. No table has a column for a URL or a title.

### Two ways in

1. **Heartbeats** (macOS, Linux). The collector calls `heartbeat(at, activity)` every 5 s and on every focus change. The core extends the open span while the activity key `(app_id, domain)` stays the same and heartbeats keep arriving within `MAX_GAP` (30 s). A different key or a gap closes the span and opens a new one. `heartbeat(at, None)` closes the open span. When an idle timeout fires, the collector calls `away_since(last_input, now)` instead: the user really left at their last input, so every heartbeat span is cut back to it. Heartbeats kept coming while the user was idle, and a site change or an expired hint in that time split them into several spans, not just the open one. A span crossing `last_input` ends there, and a span after it becomes zero-length at `last_input`. Each cut gets a new `rev`, so it syncs like any update. Interval spans are left alone.
2. **Intervals** (Android). UsageStats already gives exact foreground intervals, so the collector calls `record_interval(start, end, app)`. Each piece's span id is a hash of `(device, app_id, piece start)`, and the first piece's id is the interval's id, so re-ingesting the same window is a no-op. Calling it again with the same `(app_id, start)` and a later `end` updates that span in place with a new `rev`. The Android collector relies on this to grow a session that is still in the foreground at each ingest. A call with `end <= start` retracts the interval with that `(app_id, start)`: every stored piece becomes a zero-length span with a new `rev`. The collector uses this when `DEVICE_STARTUP` shows that a session it recorded up to "now" never ran past its start. With no such interval the call does nothing. The store remembers which interval wrote each piece (a local column that never syncs). When a re-ingest splits the interval differently, because a hint was pruned or UsageStats reported a shorter end, every earlier piece missing from the new split becomes a zero-length span with a new `rev`. That tombstone syncs like any update, and summaries skip zero-length spans.

A heartbeat stamped before the latest `end_ms` this device has stored (the clock stepped back, or a callback arrived late) never opens an overlapping span, even when that span is already closed by `flush` or `away_since`. With the open span's key it is dropped. Otherwise its span starts at that latest end. The spans table is the high-water mark, so a cut by `away_since` lowers it to `last_input`, or to the latest interval end when that is later.

App ids and app names are cut to 512 bytes, and the device name to 256 bytes, on a char boundary. A host longer than 253 bytes, the DNS limit, counts as no site. This bounds the size of one span on the wire, and of the device record each batch carries.

### Web attribution

Hints apply only when the span's app is a browser according to the core's browser table (`core/src/apps.rs`). A heartbeat whose activity already carries `web` (macOS AppleScript) skips the hint lookup. Which hints count depends on the path:

| Path | Hints for the app's own canonical browser | Hints naming a browser not in the table | Age limit |
|---|---|---|---|
| Heartbeat | yes | yes, for any focused browser | at most `HINT_TTL` (60 s) before the heartbeat |
| Interval, first piece | yes | no | at most 30 minutes before the interval start |
| Interval, split points | yes | no | strictly inside the interval |

An interval splits only where the site changes. A split hint with the same domain as the piece before it, such as the extension re-sending the open page every 10 s, adds no split, and neighbouring pieces with the same domain (or both with no page) are one span.

The unknown-browser fallback exists for an extension running in a Chromium fork it cannot identify, which reports `chromium-family` (see Extension protocol). Intervals never use it, because Android knows exactly which package was in front, and a Kiwi page must not label Chrome time.

When the hint a lookup picks is a no-page hint, the span gets no page, on both paths: a heartbeat gets `web: None`, and an interval piece starting at it, or a first piece that takes it, has no site. That ends the previous site's time at once instead of up to 60 s later.

Hints stamped more than a day ahead of the system clock are dropped. Hints are kept for 7 days, counted back from the newest hint or from now, whichever is earlier, and pruned on open and on every new hint.

`domain` is the URL host with `www.`, `m.` and `mobile.` stripped.

### Canonical apps

The same app has different ids per platform (`com.google.Chrome`, `google-chrome`, `com.android.chrome`). `apps.rs` maps known ids to one canonical key and display name so the dashboard can group "Chrome" across devices. Unknown ids stay as they are.

## Privacy

- **Only apps, domains, times and device names are stored.** Spent Day never stores or syncs a full URL or a page title, and it never keeps, stores or sends a window title. `Activity` and `WebPage` have no title field, so no collector can hand one to the core. On Sway, the window tree the Linux collector reads (`swaymsg -t get_tree`) includes every window's title. On Hyprland, `hyprctl activewindow -j` includes the focused window's title. The collector parses the reply in memory, takes the app id and ignores the rest. A `WebPage` URL is reduced to its domain in `web::normalize_domain` the moment it reaches the core, and the URL is dropped. The extension sends only the page's origin, so the path, query and fragment never leave the browser. `core/tests/privacy.rs` and the sync tests check that a URL's path, query and fragment, and a title sent to the listener, reach neither the database file nor a sync batch.
- **The database is private to its owner.** On Unix the database holds the sync code in plain text, so `open` creates missing directories `0700` and sets the database and its `-wal`, `-shm` and `-journal` files to `0600`, existing ones too, whatever the umask. Only the directories that call creates become `0700`, and directories that already exist are left alone. A database path that is a symlink or a `file:` URI is refused, because SQLite would put the `-wal` and `-shm` files where they are not protected, and so is a sidecar that is a symlink. The core creates the file with `O_EXCL`, which never follows a symlink, and changes each existing file through an open handle only when it is the inode `lstat` saw. A database file or sidecar the core can't read, such as one set to `0200`, makes `open` fail with an error that names the file and says to run `chmod 600` on it. The core doesn't change it by path, because `chmod` would follow a symlink swapped in after the check. The resolved path stays a byte path all the way to SQLite, so a folder whose name is not UTF-8 still opens the file that was checked. SQLite opens the path with its folders resolved and `SQLITE_OPEN_NOFOLLOW`, so a symlink swapped in after the check is not followed either. If any of this fails, `open` fails. `open_existing` sets the files to `0600` and leaves the directory alone.
- **Private windows report no page.** The collector or extension decides what a private window reports, and sends no page for one. See the per-platform notes.

## Summary

`summary(SummaryQuery { start_ms, end_ms, bucket_ms, device_id })` returns totals clipped to the range:

- `total_ms`, `by_device`, `apps` (each with its `domains` when the app is a browser), `sites` (domains across all browsers), `buckets` (time per `bucket_ms` slot from `start_ms`).
- A browser's `domains` holds its time with no site recorded (no hint, a stale hint, or a non-http(s) page) as one row with an empty `domain`, sorted with the rest. Apps show it as "Other". `sites` never has an empty `domain`.
- Each app row lists `app_ids`, the distinct platform ids that added time to it, most time first, so the macOS app can find the icon for `com.google.Chrome` under the canonical `chrome` row.

The UI passes local-midnight boundaries, so the core never deals with time zones. Time on two devices at once counts twice. That is deliberate: "screen time" is per screen.

## Sync

- One random 32-byte **sync secret** per account, shown to the user as a sync code. Joining a device means pasting the code.
- The code is 56 Crockford base32 symbols in 14 groups of 4: the 256 secret bits, then a 24-bit check made of the first 3 bytes of SHA-256 of the secret. A mistyped code passes the check about once in 16.7 million, so a typo fails with `InvalidSyncCode` instead of quietly joining a new, empty account. Input is case-insensitive, ignores spaces and dashes, and reads `O` as `0` and `I` and `L` as `1`.
- HKDF-SHA256, with the salt `spentday-sync-v1`, derives `space_id` (server namespace), `auth_token` (bearer token, the server stores its SHA-256 on first use), and `enc_key`.
- A **batch** is JSON `{ device, spans }`, encrypted with XChaCha20-Poly1305 (random nonce, AAD = `space_id || device_id`). The server never sees app names, domains or device names.
- The server URL must be `https://`, unless the host is loopback, `localhost` or a private IP literal (10/8, 172.16/12, 192.168/16, fc00::/7). Plain `http://` to any other host fails with `InvalidArgument`, `.local` names included: a name over plain http would go wherever mDNS or DNS answers, and checking every answer would mean resolving the name in the core and dialing the checked address. Plain `http://localhost` is stored as `http://127.0.0.1`, so no lookup happens for it. A server that listens only on `[::1]` must be entered as `http://[::1]:<port>`. The client follows no redirects, so a server cannot move it to plain `http://` or to an address the rule refuses. A 3xx answer is an error.
- `rev` comes from `MAX(rev) + 1` inside the write transaction (`BEGIN IMMEDIATE`), so two handles on one database file never reuse a rev.
- `sync_now()` pushes local spans with `rev > pushed_rev`, then pulls batches after the stored server cursor and upserts them. Both directions are idempotent, so a crash mid-sync just repeats work.
- A push batch holds at most 2000 spans and at most about 3 MiB of plaintext JSON, which seals to about 4 MiB, half the server's body cap. It always holds at least one span, and the field limits above keep one span far below the cap.
- `sync_now()` calls on one `SpentDay` run one at a time. A second call waits for the first, then runs, so a UI sync and a background sync never push the same batch twice.
- `sync_now()` still pulls when the push fails, then returns the push error, so one device that cannot upload still receives everyone else's data.
- A pulled batch that will not decrypt or parse is skipped for good. The cursor moves past it, and `SyncReport::skipped_batches` counts it. A device `platform` this build does not know reads as `linux` instead of failing the batch.
- **Cursor recovery.** Cursors are not authenticated, so a server or anything between it and the device can send a batch that fails to open with a cursor far ahead, such as `i64::MAX`. Skipping it moves the stored cursor there, and without recovery the device would ask for batches after it forever. So every pull response carries `head`, the newest cursor the server holds for the space. When the stored cursor is past `head`, the device resets it to 0 and reads the whole space again. Merging is idempotent by `rev`, so the rescan only costs bandwidth. It happens at most once per `sync_now`, so a server that keeps sending a low `head` cannot loop a sync. A server that keeps lying can still withhold batches, which it can always do.
- `set_sync_config` writes the config, resets `pushed_rev` and the pull cursor, and bumps a sync generation in one transaction. Every position write and merge checks the generation its sync started with and is dropped on a mismatch, so a sync still running for the old account writes nothing into the new one. Each push batch is read in the same read transaction as that check, so the old account never receives activity recorded after the switch.

### Server API

```
POST /v1/spaces/{space_id}/batches      Authorization: Bearer <auth_token hex>
     { "device_id": "...", "nonce": "<b64>", "ciphertext": "<b64>" }  ->  { "cursor": 42 }
GET  /v1/spaces/{space_id}/batches?after=0&limit=200
     -> { "batches": [{ "cursor", "device_id", "nonce", "ciphertext" }], "more": false, "head": 42 }
GET  /health -> "ok"
```

`head` is the newest cursor in the space, 0 when it is empty. Request bodies are capped at 8 MiB. A page holds at most 200 batches (`limit` is clamped to 1..200) and stops before its batches pass 4 MiB, but always holds at least one, so no page is larger than about 8 MiB. The client reads responses up to 16 MiB.

## Platform collectors

No platform keeps window titles or page titles. On Sway, the window tree the Linux collector reads includes every window's title. On Hyprland, the reply includes the focused window's title. The collector takes the app id and ignores the rest.

| Platform | App in front | Website | Away |
|---|---|---|---|
| macOS | `NSWorkspace` activation notifications + 5 s timer | AppleScript for Safari and Chromium browsers. It checks whether the front window is private (Safari can't say), then reads the URL of its active tab. Extension for Firefox. | `CGEventSource` idle > 3 min, unless a power assertion says media is playing. Sleep and lock notifications. |
| Linux | X11 `_NET_ACTIVE_WINDOW`. Wayland via GNOME Shell extension, KWin script, Sway and Hyprland IPC. App id only. | Extension | XScreenSaver, Mutter IdleMonitor or `ext-idle-notify-v1` idle > 3 min, unless an idle inhibitor is held or the focused app's MPRIS player is playing, checked every 5 s on a thread of its own. A check that found media keeps the user present for 30 s, and checks that find nothing or cannot tell do not renew it. Away starts at the later of the last input and the last check that found media. |
| Android | `UsageStatsManager` events, ingested every 15 min and on app open | Accessibility service reading the URL bar of known browsers (optional) | Implicit, UsageStats only counts foreground time |

iOS is out of scope for now, because Apple doesn't let apps export Screen Time data.

## Not yet

Web dashboard, compaction of old server batches, per-domain privacy filters, mapping apps to services (YouTube app and youtube.com as one row), Windows, iOS.

## Extension protocol

The extension sends the active tab of the focused browser window every 10 s and on every tab, URL or focus change. When focus leaves the browser it sends one `"url": null` for itself, because the newly focused window may be a private one it cannot see. It sends the same when a report finds no browser window focused. Either way only when the app may still have a page open: the last report it meant to send or the last one the app took named a page. It keeps both in `storage.session`, so a service worker the browser restarted still ends the page. The app has taken a report only when it answers 2xx. Until it takes the latest one, every alarm, interval and event sends that one again, focused or not, so an error answer, a network error, the 5 s timeout or an answer lost on the way back cannot leave a page open. After the app takes the `"url": null`, it sends nothing until a browser window has focus again. It sends only the tab's origin, such as `https://www.youtube.com`, never its path, query, fragment or title. When the active tab is not an http(s) page, or the window is private, it sends `"url": null`, which the listener records as a no-page hint. One request is out at a time, and at most one more waits. A newer report replaces the waiting one, so an app that stops answering never builds a backlog of stale pages ahead of the latest. A report that started before the latest focus change is dropped, so a slow tab lookup cannot undo the `"url": null` sent when focus left.

```
POST http://127.0.0.1:7853/v1/web
{ "browser": "firefox", "url": "https://www.youtube.com" }  ->  204
{ "browser": "firefox", "url": null }  ->  204   (no-page hint)
POST http://127.0.0.1:7853/v1/health  ->  200 { "ok": true, "device": "<device name>" }   (GET works too)
```

The popup calls `/v1/health` to test the connection. It uses POST, because browsers always send an `Origin` with a POST, and the listener answers only requests with an extension `Origin`. It follows the same `Host`, origin and CORS rules as `POST /v1/web`.

`browser` is a canonical key from `core/src/apps.rs`: `chrome`, `firefox`, `safari`, `chromium`, `brave`, `edge`, `opera`, `vivaldi`, `arc`, `zen`. A Chromium fork reports its own key (`brave`, `edge`, `opera`, `vivaldi`, `arc`) when it can tell which fork it runs in, and `chromium-family` otherwise. `chromium-family` is deliberately not in the table, so the heartbeat path attributes it to whichever browser is focused. Hints only attribute time to the browser they name, so a fork that reports `chrome` or `chromium` labels only Chrome's or Chromium's time, not its own.

The listener stamps the receive time and keeps only the domain of `url`. It ignores non-http(s) URLs and any other field, such as a `title`. It serves a request only when `Host` is `127.0.0.1:<port>`, `localhost:<port>` or `[::1]:<port>`, which stops a web page that rebinds its own domain to 127.0.0.1, and when `Origin` starts with `chrome-extension://` or `moz-extension://`. Anything else gets 403, a request with no `Origin` too. A web page cannot set an extension `Origin`, so no page passes. A local program can send any `Origin`, so these checks do not stop other programs on the computer, or another extension. It serves one request per connection, each on its own thread, at most 16 at once. A connection past that gets 503 before anything of it is read. A client has 3 s to send its whole request, so a client that stalls holds its slot for at most 3 s.

## Threat model

- **The sync server** learns each space id, the device ids that write to it, the IP address each device connects from, when each device syncs, and how large each encrypted batch is. It cannot read app names, domains or device names. It can withhold batches, replay old ones, or drop a space, and a device cannot tell. It can also send a cursor far ahead, which costs the device one rescan (see Cursor recovery).
- **Anyone with the sync code** can read every device's data in the space and write batches as any device. Batches are authenticated with the shared key, not per device, so a device holding the code can write spans that claim to come from another device in the space.
- **Other programs on the same computer** can send reports to the extension listener, and the core cannot tell them from the extension. If Spent Day is not running, another local user can bind port 7853 first and receive the domains the extension reports.
- **Anyone who can read the database file** gets the sync code. The core keeps the file private to its owner on Unix, but the code is not encrypted at rest.
