// Runs background.js against a fake browser and checks what it posts to the Spent Day app.
// node --test extension/test

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const source = readFileSync(new URL("../background.js", import.meta.url), "utf8");
const ENDPOINT = "http://127.0.0.1:7853/v1/web";
const CHROME = { userAgentData: { brands: [{ brand: "Google Chrome" }, { brand: "Chromium" }] } };
const NORMAL = { id: 1, focused: true, incognito: false };

// `win` is what windows.getLastFocused resolves to, or an Error it rejects with.
async function posts({ win = NORMAL, tab, navigator = CHROME, browserInfo, then }) {
  const sent = [];
  const event = () => ({ addListener() {} });
  const focusListeners = [];
  const context = {
    chrome: {
      windows: {
        getLastFocused: async () => {
          if (win instanceof Error) throw win;
          return win;
        },
        onFocusChanged: { addListener: (listener) => focusListeners.push(listener) },
        WINDOW_ID_NONE: -1,
      },
      tabs: { query: async () => (tab ? [tab] : []), onActivated: event(), onUpdated: event() },
      runtime: { onStartup: event(), onInstalled: event() },
      alarms: { create() {}, onAlarm: event() },
      storage: { session: { get: async () => ({}), set: async () => {} } },
    },
    navigator: { userAgent: "Mozilla/5.0 (X11; Linux x86_64) Chrome/141.0.0.0 Safari/537.36", ...navigator },
    fetch: async (url, init) => {
      sent.push({ url, body: JSON.parse(init.body) });
      return { ok: true };
    },
    AbortSignal,
    URL,
    setInterval() {},
  };
  if (browserInfo) context.browser = { runtime: { getBrowserInfo: async () => browserInfo } };
  vm.runInNewContext(source, context);
  await new Promise((resolve) => setTimeout(resolve, 20));
  if (then) {
    then(focusListeners);
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  return sent;
}

test("an http(s) tab reports its origin only, never its path, query, fragment or title", async () => {
  const tab = { url: "https://github.com/spentday/secret-path?q=secret-query#secret-fragment", title: "Secret Title" };
  assert.deepEqual(await posts({ tab }), [{ url: ENDPOINT, body: { browser: "chrome", url: "https://github.com" } }]);
  assert.deepEqual(await posts({ tab: { url: "http://127.0.0.1:8000/alpha.html", title: "Alpha" } }), [
    { url: ENDPOINT, body: { browser: "chrome", url: "http://127.0.0.1:8000" } },
  ]);
});

test("a private window reports no page and no title", async () => {
  const tab = { url: "https://secret.example/", title: "Secret", incognito: true };
  assert.deepEqual(await posts({ win: { ...NORMAL, incognito: true }, tab }), [
    { url: ENDPOINT, body: { browser: "chrome", url: null } },
  ]);
  assert.deepEqual(await posts({ tab }), [{ url: ENDPOINT, body: { browser: "chrome", url: null } }]);
});

test("a tab with no web page reports no page", async () => {
  for (const url of ["chrome://newtab/", "about:blank", "file:///home/me/report.pdf", undefined]) {
    assert.deepEqual(await posts({ tab: { url, title: "New Tab" } }), [
      { url: ENDPOINT, body: { browser: "chrome", url: null } },
    ]);
  }
});

test("nothing is sent while no browser window is focused", async () => {
  const tab = { url: "https://github.com/", title: "GitHub" };
  assert.deepEqual(await posts({ win: { ...NORMAL, focused: false }, tab }), []);
  assert.deepEqual(await posts({ win: new Error("No last-focused window"), tab }), []);
  assert.deepEqual(await posts({ tab: undefined }), []);
});

test("each browser reports its canonical key", async () => {
  const tab = { url: "https://github.com/", title: "GitHub" };
  const brands = (...names) => ({ userAgentData: { brands: names.map((brand) => ({ brand })) } });
  const cases = [
    [{ navigator: brands("Chromium", "Not.A/Brand") }, "chromium-family"],
    [{ navigator: brands("Microsoft Edge", "Chromium") }, "edge"],
    [{ navigator: brands("Opera", "Chromium") }, "opera"],
    [{ navigator: { ...brands("Brave", "Chromium"), brave: {} } }, "brave"],
    [{ navigator: brands("Chromium"), tab: { ...tab, vivExtData: "{}" } }, "vivaldi"],
    [{ navigator: {}, browserInfo: { name: "Firefox", vendor: "Mozilla" } }, "firefox"],
    [{ navigator: {}, browserInfo: { name: "Zen", vendor: "Zen" } }, "zen"],
    [{ navigator: {}, browserInfo: { name: "LibreWolf" } }, "librewolf"],
  ];
  for (const [setup, browser] of cases) {
    const sent = await posts({ tab, ...setup });
    assert.deepEqual(sent.map((p) => p.body.browser), [browser], JSON.stringify(setup));
  }
});

test("focus leaving the browser ends its page, so a private window takes no credit", async () => {
  const tab = { url: "https://github.com/", title: "GitHub" };
  const sent = await posts({
    tab,
    then: (listeners) => listeners.forEach((listener) => listener(-1)),
  });
  assert.deepEqual(
    sent.map((p) => p.body),
    [
      { browser: "chrome", url: "https://github.com" },
      { browser: "chrome", url: null },
    ],
  );
});

test("focus leaving before any report sends nothing", async () => {
  const sent = await posts({
    win: { ...NORMAL, focused: false },
    then: (listeners) => listeners.forEach((listener) => listener(-1)),
  });
  assert.deepEqual(sent, []);
});

const PAGE = { url: "https://github.com/spentday?tab=1", title: "GitHub" };
// What the app hears for PAGE.
const SENT = { browser: "chrome", url: "https://github.com" };
const settle = () => new Promise((resolve) => setTimeout(resolve, 20));

// One browser whose service worker can be started more than once. Each `start()` runs
// background.js in a fresh context, as when the browser restarts a stopped worker, sharing
// the browser's windows, session storage and what reached the app.
function browser({ tab = PAGE } = {}) {
  const shared = {
    focused: true,
    sent: [],
    storage: {},
    tabQuery: async () => [tab],
    fetch: async () => ({ ok: true }),
    timeouts: [],
  };
  shared.start = () => {
    const listeners = { focus: [], activated: [], alarm: [] };
    const event = () => ({ addListener() {} });
    vm.runInNewContext(source, {
      chrome: {
        windows: {
          getLastFocused: async () => ({ ...NORMAL, focused: shared.focused }),
          onFocusChanged: { addListener: (listener) => listeners.focus.push(listener) },
          WINDOW_ID_NONE: -1,
        },
        tabs: {
          query: () => shared.tabQuery(),
          onActivated: { addListener: (listener) => listeners.activated.push(listener) },
          onUpdated: event(),
        },
        runtime: { onStartup: event(), onInstalled: event() },
        alarms: { create() {}, onAlarm: { addListener: (listener) => listeners.alarm.push(listener) } },
        storage: {
          session: {
            get: async (key) => (key in shared.storage ? { [key]: structuredClone(shared.storage[key]) } : {}),
            set: async (items) => Object.assign(shared.storage, structuredClone(items)),
          },
        },
      },
      navigator: { userAgent: "", ...CHROME },
      fetch: (url, init) => {
        shared.sent.push(JSON.parse(init.body));
        return shared.fetch(init);
      },
      AbortSignal: {
        timeout: (ms) => {
          const controller = new AbortController();
          shared.timeouts.push({ ms, expire: () => controller.abort() });
          return controller.signal;
        },
      },
      URL,
      setInterval() {},
    });
    return {
      focusChanged: (windowId) => listeners.focus.forEach((listener) => listener(windowId)),
      tabActivated: () => listeners.activated.forEach((listener) => listener({})),
      alarm: () => listeners.alarm.forEach((listener) => listener({ name: "report" })),
    };
  };
  return shared;
}

test("a report still waiting on the tab when focus leaves does not undo the clear", async () => {
  const chrome = browser();
  const worker = chrome.start();
  await settle();
  let release;
  chrome.tabQuery = () => new Promise((resolve) => (release = resolve));
  worker.tabActivated();
  await settle();
  chrome.focused = false;
  worker.focusChanged(-1);
  await settle();
  release([PAGE]);
  await settle();
  assert.deepEqual(chrome.sent, [SENT, { browser: "chrome", url: null }]);
});

test("a send waits for the one before it", async () => {
  const chrome = browser();
  let answer;
  chrome.fetch = () => new Promise((resolve) => (answer = resolve));
  const worker = chrome.start();
  await settle();
  chrome.focused = false;
  worker.focusChanged(-1);
  await settle();
  assert.deepEqual(chrome.sent, [SENT]);
  chrome.fetch = async () => ({ ok: true });
  answer({ ok: true });
  await settle();
  assert.deepEqual(chrome.sent, [SENT, { browser: "chrome", url: null }]);
});

test("a send with no answer gives up after 5 s, so the next one still goes out", async () => {
  const chrome = browser();
  chrome.fetch = (init) =>
    new Promise((_resolve, reject) => init.signal?.addEventListener("abort", () => reject(new Error("timed out"))));
  const worker = chrome.start();
  await settle();
  chrome.fetch = async () => ({ ok: true });
  chrome.focused = false;
  worker.focusChanged(-1);
  await settle();
  assert.deepEqual(chrome.sent, [SENT]);
  assert.deepEqual(chrome.timeouts.map((t) => t.ms), [5000]);
  chrome.timeouts[0].expire();
  await settle();
  assert.deepEqual(chrome.sent, [SENT, { browser: "chrome", url: null }]);
});

test("a restarted worker still ends the page when focus leaves", async () => {
  const chrome = browser();
  chrome.start();
  await settle();
  chrome.focused = false;
  chrome.start().focusChanged(-1);
  await settle();
  assert.deepEqual(chrome.sent, [SENT, { browser: "chrome", url: null }]);
});

test("a worker that starts with no browser window focused ends the page still open", async () => {
  const chrome = browser();
  chrome.start();
  await settle();
  chrome.focused = false;
  chrome.start();
  await settle();
  assert.deepEqual(chrome.sent, [SENT, { browser: "chrome", url: null }]);
});

test("a worker that starts unfocused after a tab with no page sends nothing more", async () => {
  const chrome = browser({ tab: { url: "chrome://newtab/", title: "New Tab" } });
  chrome.start();
  await settle();
  chrome.focused = false;
  chrome.start();
  await settle();
  assert.deepEqual(chrome.sent, [{ browser: "chrome", url: null }]);
});

test("100 reports to an app that never answers keep one send out, and the clear goes next", async () => {
  const chrome = browser();
  let outstanding = 0;
  let most = 0;
  chrome.fetch = (init) => {
    most = Math.max(most, ++outstanding);
    return new Promise((_resolve, reject) =>
      init.signal.addEventListener("abort", () => {
        outstanding--;
        reject(new Error("timed out"));
      }),
    );
  };
  const worker = chrome.start();
  await settle();
  for (let i = 0; i < 100; i++) {
    const tab = { url: `https://stale${i}.example/`, title: `Stale ${i}` };
    chrome.tabQuery = async () => [tab];
    worker.tabActivated();
    await settle();
  }
  chrome.focused = false;
  worker.focusChanged(-1);
  await settle();
  assert.deepEqual(chrome.sent, [SENT]);
  chrome.fetch = async () => ({ ok: true, status: 200 });
  chrome.timeouts[0].expire();
  await settle();
  assert.deepEqual(chrome.sent, [SENT, { browser: "chrome", url: null }]);
  assert.equal(most, 1);
});

test("a clear that failed is sent again by a restarted worker until the app takes it", async () => {
  const chrome = browser();
  const worker = chrome.start();
  await settle();
  chrome.fetch = async () => {
    throw new TypeError("Failed to fetch");
  };
  chrome.focused = false;
  worker.focusChanged(-1);
  await settle();
  chrome.fetch = async () => ({ ok: true, status: 200 });
  chrome.start();
  await settle();
  chrome.start().alarm();
  await settle();
  assert.deepEqual(chrome.sent, [
    SENT,
    { browser: "chrome", url: null },
    { browser: "chrome", url: null },
  ]);
});

test("an app that answers with an error has not taken the report", async () => {
  const chrome = browser();
  const worker = chrome.start();
  await settle();
  chrome.fetch = async () => ({ ok: false, status: 503 });
  chrome.focused = false;
  worker.focusChanged(-1);
  await settle();
  chrome.fetch = async () => ({ ok: true, status: 200 });
  worker.alarm();
  await settle();
  worker.alarm();
  await settle();
  assert.deepEqual(chrome.sent, [
    SENT,
    { browser: "chrome", url: null },
    { browser: "chrome", url: null },
  ]);
});

test("a clear is sent again until the app takes it, even when the page's answer was lost", async () => {
  const chrome = browser();
  const lost = async () => {
    throw new TypeError("Failed to fetch");
  };
  chrome.fetch = lost;
  const worker = chrome.start();
  await settle();
  chrome.focused = false;
  worker.focusChanged(-1);
  await settle();
  worker.alarm();
  await settle();
  chrome.fetch = async () => ({ ok: true, status: 200 });
  const restarted = chrome.start();
  await settle();
  const clear = { browser: "chrome", url: null };
  assert.deepEqual(chrome.sent, [SENT, clear, clear, clear]);
  restarted.alarm();
  await settle();
  chrome.start();
  await settle();
  assert.deepEqual(chrome.sent, [SENT, clear, clear, clear]);
});
