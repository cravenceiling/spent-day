// Reports the site of the active tab of the focused browser window to the Spent Day app on
// 127.0.0.1. It sends only the page's origin, such as https://www.youtube.com. The path, query,
// fragment and title never leave the browser. The core drops reports older than 60 s, so this
// sends every 10 s and on every change.

const ENDPOINT = "http://127.0.0.1:7853/v1/web";
const INTERVAL_MS = 10_000;
// Only one send is out at a time, so one the app never answers must not hold up the next.
const SEND_TIMEOUT_MS = 5_000;

// Gecko browsers name themselves: "Firefox", "Zen", "LibreWolf". A name the core's browser
// table (core/src/apps.rs) does not know counts for whichever browser is focused.
const geckoKey = globalThis.browser?.runtime?.getBrowserInfo?.().then(
  (info) => info.name.split(/\s/)[0].toLowerCase(),
  () => "firefox",
);

// Canonical browser keys from core/src/apps.rs. A hint only counts toward the browser it
// names, so a fork that cannot tell which one it is reports chromium-family, a key the table
// leaves out on purpose.
async function browserKey(tab) {
  if (geckoKey) return geckoKey;
  if (navigator.brave) return "brave";
  // Vivaldi sends neither a brand nor a user agent token, but adds vivExtData to its tabs.
  if (tab.vivExtData !== undefined || /Vivaldi\//.test(navigator.userAgent)) return "vivaldi";
  const brands = (navigator.userAgentData?.brands ?? []).map((b) => b.brand);
  const known = [
    ["Microsoft Edge", /Edg\//, "edge"],
    ["Opera", /OPR\//, "opera"],
    ["Google Chrome", null, "chrome"],
  ];
  for (const [brand, pattern, key] of known) {
    if (brands.includes(brand) || pattern?.test(navigator.userAgent)) return key;
  }
  return "chromium-family";
}

async function focused() {
  const win = await chrome.windows.getLastFocused();
  if (!win?.focused) return null;
  const [tab] = await chrome.tabs.query({ active: true, windowId: win.id });
  return tab ? { win, tab } : null;
}

// The page to report as its origin, or null when the tab shows none: a private window, a new
// tab, chrome://, about:, or a local file.
function page(win, tab) {
  if (win.incognito || tab.incognito) return null;
  if (!/^https?:\/\//.test(tab.url ?? "")) return null;
  try {
    return { url: new URL(tab.url).origin };
  } catch {
    return null;
  }
}

// What the app should hear (`desired`), and the last report it took with a 2xx answer
// (`acknowledged`). Focus leaving goes to another app or to a private window this extension
// may not see, so it ends the page for this browser only. Kept in session storage, because the
// browser stops an idle worker and the focus change that ends the page may be what starts the
// next one.
let sender = { desired: null, acknowledged: null };
const restored = chrome.storage.session.get("sender").then(
  (saved) => {
    if (saved.sender) sender = saved.sender;
  },
  () => {},
);

// Bumped by every focus change, so a report that started before one changes nothing.
let generation = 0;

// One send is out at a time, so an older report cannot reach the app after a newer one. Every
// alarm, interval and event then sends `desired` while the app has not taken it, focused or
// not, so a send that failed or whose answer was lost goes again until the app takes it. A
// report also sets `owed`, which sends it even when the app took the same page, because the
// core counts a page only for 60 s after its report.
let sending = null;
let owed = false;

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

function want(body) {
  sender.desired = body;
  chrome.storage.session.set({ sender }).catch(() => {});
  if (!same(body, sending)) owed = true;
}

async function deliver() {
  if (sending || (!owed && same(sender.desired, sender.acknowledged))) return;
  owed = false;
  const body = (sending = sender.desired);
  const taken = await fetch(ENDPOINT, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(SEND_TIMEOUT_MS),
  }).then(
    (response) => response.ok,
    () => false,
  );
  sending = null;
  if (taken) {
    sender.acknowledged = body;
    chrome.storage.session.set({ sender }).catch(() => {});
  } else if (same(sender.desired, body)) {
    return;
  }
  deliver();
}

// A page the app may still have open, from a report it took or one that may have reached it.
function endPage() {
  const open = [sender.desired, sender.acknowledged].find((body) => body?.url);
  if (open) want({ browser: open.browser, url: null });
}

// With no browser window focused, a page still open from an earlier report ends.
async function report() {
  const started = generation;
  await restored;
  const front = await focused().catch(() => null);
  const browser = front && (await browserKey(front.tab));
  if (started === generation && !front) {
    endPage();
  } else if (started === generation) {
    const shown = page(front.win, front.tab);
    want(shown ? { browser, ...shown } : { browser, url: null });
  }
  deliver();
}

async function focusLeft() {
  const started = generation;
  await restored;
  if (started === generation) endPage();
  deliver();
}

chrome.tabs.onActivated.addListener(report);
chrome.tabs.onUpdated.addListener((_id, change, tab) => {
  if (tab.active && change.url) report();
});
chrome.windows.onFocusChanged.addListener((windowId) => {
  generation++;
  if (windowId === chrome.windows.WINDOW_ID_NONE) focusLeft();
  else report();
});
chrome.runtime.onStartup.addListener(report);
chrome.runtime.onInstalled.addListener(report);

// Alarms fire at most every 30 s but restart a stopped service worker. While it runs, the
// interval's extension API calls keep it alive.
chrome.alarms.create("report", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(report);
setInterval(report, INTERVAL_MS);
report();
