# Spent Day browser extension

A WebExtension for Chrome, Chromium-based browsers and Firefox on the desktop. It tells the Spent Day app which site is in front.

On macOS, Chrome-based browsers report their pages without it. You need the extension for Firefox on macOS, and for every browser on Linux.

Load this folder unpacked: in Chrome, open `chrome://extensions`, turn on Developer mode and click Load unpacked. In Firefox, open `about:debugging`, then This Firefox, then Load Temporary Add-on, and pick `manifest.json`. `build.sh` builds the store zips.

It talks only to `127.0.0.1:7853`, and it never reports private windows. It sends only the origin of the tab in front, such as `https://www.youtube.com`, never the rest of the address or the page title.

The app listens on `127.0.0.1:7853` for these reports. It refuses web pages, but any program on the computer, and any other extension, can send it reports too, and the app can't tell them apart. If Spent Day isn't running, another user on the same computer could listen on that port first and receive the sites the extension reports.

Run the tests with `node --test extension/test/*.test.mjs` from the repository root.
