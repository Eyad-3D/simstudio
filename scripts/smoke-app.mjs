#!/usr/bin/env node
/**
 * Smoke-test the PACKAGED desktop app.
 *
 * Launches the real executable with remote debugging on, waits for the window
 * to navigate off the splash screen, and checks the React app rendered against
 * a live backend. Catches shell-level breakage — a backend that never starts,
 * a window stuck on "Starting the simulation engine…", a blank page — that no
 * backend-only test can see.
 *
 * Needs a display; run under xvfb on a headless machine:
 *   xvfb-run -a node scripts/smoke-app.mjs desktop/release/linux-unpacked/simstudio
 */
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";

const exe = process.argv[2];
const DEBUG_PORT = 9333;

if (!exe || !existsSync(exe)) {
  console.error(`✗ no packaged app at ${exe ?? "(no path given)"}`);
  process.exit(1);
}

let log = "";
const child = spawn(exe, [
  "--no-sandbox",
  `--remote-debugging-port=${DEBUG_PORT}`,
  "--remote-allow-origins=*",
]);
child.stdout.on("data", (d) => (log += d));
child.stderr.on("data", (d) => (log += d));

const stop = () => { try { child.kill(); } catch { /* already gone */ } };
const fail = (msg) => {
  console.error(`✗ ${msg}\n\n--- app output ---\n${log.slice(-3000)}`);
  stop();
  process.exit(1);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Wait for a devtools page target that has left the splash screen. */
async function waitForPage(timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) fail(`app exited early (code ${child.exitCode})`);
    try {
      const targets = await (await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/list`)).json();
      const page = targets.find((t) => t.type === "page" && t.url.startsWith("http://127.0.0.1"));
      if (page) return page;
    } catch { /* devtools not up yet */ }
    await sleep(1000);
  }
  fail(`app never loaded its UI within ${timeoutMs / 1000}s (still on the splash?)`);
}

/** Evaluate an expression in the page over the devtools protocol. */
function evaluate(page, expression) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(page.webSocketDebuggerUrl);
    const timer = setTimeout(() => reject(new Error("devtools evaluate timed out")), 30_000);
    ws.onopen = () => ws.send(JSON.stringify({
      id: 1,
      method: "Runtime.evaluate",
      params: { expression, returnByValue: true },
    }));
    ws.onerror = () => { clearTimeout(timer); reject(new Error("devtools socket failed")); };
    ws.onmessage = (e) => {
      clearTimeout(timer);
      const msg = JSON.parse(e.data);
      ws.close();
      resolve(msg.result?.result?.value);
    };
  });
}

try {
  const page = await waitForPage();
  console.log(`✓ window loaded: ${page.url}`);

  // Give React a moment to mount and fetch its first data.
  await sleep(5000);

  const probe = JSON.parse(await evaluate(page, `JSON.stringify({
    mounted: (document.getElementById('root')?.children.length ?? 0) > 0,
    nodes: document.querySelectorAll('.react-flow__node').length,
    // The status bar reports the engine connection, and it lives at the very
    // end of the page, so test the whole text and keep a short excerpt only
    // for the failure message.
    connected: /backend connected/i.test(document.body.innerText),
    excerpt: document.body.innerText.replace(/\s+/g, ' ').slice(0, 400),
  })`));

  if (!probe.mounted) fail("the React app did not mount");
  console.log("✓ UI mounted");

  if (probe.nodes < 1) fail("no topology nodes rendered — the example project did not load");
  console.log(`✓ example project rendered: ${probe.nodes} elements`);

  // A packaged app that cannot reach its own backend is the failure this
  // whole script exists to catch.
  if (!probe.connected) {
    fail(`the app is not talking to its backend.\n\nvisible text:\n${probe.excerpt}`);
  }
  console.log("✓ backend connected");

  console.log("\npackaged app smoke test passed");
  stop();
  process.exit(0);
} catch (err) {
  fail(err.message ?? String(err));
}
