# Privacy

- Spent Day stores app names, website domains, times and device names. It never stores or syncs a full web address or a page title, and it never keeps, stores or sends a window title. On Sway, the window tree Spent Day reads includes every window's title. On Hyprland, the reply includes the focused window's title. Spent Day takes the app id and ignores the rest. The core cuts every web address it gets to its domain before storing it.
- Data stays on your devices unless you set up sync, and sync is end-to-end encrypted. [`server/README.md`](../server/README.md) says what the server can see.
- Private and incognito windows are skipped in Chrome on macOS, in Chrome and Chromium with the extension on Linux, and in Chrome and Firefox on Android. All of these were tested. On macOS, Brave, Helium, Edge, Vivaldi and Opera use Chrome's check, `mode of w is "incognito"`, and Arc checks `incognito of w`. None of these were tested.
- Safari can't tell Spent Day which windows are private, so Safari sites are off by default. If you turn them on, private browsing in Safari is recorded too.
- The sync code sits unencrypted in the local database. Anyone who gets a copy of that file can read your synced data. On macOS and Linux only your user can read the database and its folder.
- Any program on your computer can send site reports to the app's local listener. See [`extension/README.md`](../extension/README.md).
- The Android database is excluded from backups and device transfers.
