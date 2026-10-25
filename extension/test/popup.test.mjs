// Runs popup.js against a fake page and checks what it shows for each /v1/health answer.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const source = readFileSync(new URL("../popup.js", import.meta.url), "utf8");

// `answer` is what fetch resolves to, or an Error it rejects with.
async function popup(answer) {
  const elements = { status: { dataset: {} }, title: {}, detail: {} };
  const requested = [];
  vm.runInNewContext(source, {
    AbortSignal,
    document: { getElementById: (id) => elements[id] },
    fetch: async (url, init) => {
      requested.push(`${init?.method ?? "GET"} ${url}`);
      if (answer instanceof Error) throw answer;
      return answer;
    },
  });
  await new Promise((resolve) => setTimeout(resolve, 20));
  return { requested, state: elements.status.dataset.state, title: elements.title.textContent, detail: elements.detail.textContent };
}

const ok = (body) => ({ ok: true, json: async () => body });

test("a running app shows Connected and its device name", async () => {
  assert.deepEqual(await popup(ok({ ok: true, device: "thinkpad" })), {
    requested: ["POST http://127.0.0.1:7853/v1/health"],
    state: "connected",
    title: "Connected",
    detail: "Spent Day on thinkpad is recording which sites you use.",
  });
});

test("no app shows Not connected", async () => {
  const notConnected = {
    requested: ["POST http://127.0.0.1:7853/v1/health"],
    state: "disconnected",
    title: "Not connected",
    detail: "Start the Spent Day app on this computer. Sites are not recorded until it runs.",
  };
  assert.deepEqual(await popup(new TypeError("Failed to fetch")), notConnected);
  assert.deepEqual(await popup({ ok: false, json: async () => ({}) }), notConnected);
});
