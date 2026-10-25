const HEALTH = "http://127.0.0.1:7853/v1/health";

async function health() {
  try {
    // POST, because browsers always send an Origin with it, and the app answers only the extension.
    const res = await fetch(HEALTH, { method: "POST", signal: AbortSignal.timeout(2000) });
    return res.ok ? await res.json() : null;
  } catch {
    return null;
  }
}

async function show() {
  const info = await health();
  const [state, title, detail] = info?.ok
    ? ["connected", "Connected", `Spent Day on ${info.device || "this computer"} is recording which sites you use.`]
    : ["disconnected", "Not connected", "Start the Spent Day app on this computer. Sites are not recorded until it runs."];
  document.getElementById("status").dataset.state = state;
  document.getElementById("title").textContent = title;
  document.getElementById("detail").textContent = detail;
}

show();
