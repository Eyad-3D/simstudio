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
 * It also records every network request the app makes (Chromium's net log)
 * and fails if any goes beyond this computer: a clean install that has not
 * been allowed to check for updates must contact nothing outside (PLT-18).
 *
 * Needs a display; run under xvfb on a headless machine:
 *   xvfb-run -a node scripts/smoke-app.mjs desktop/release/linux-unpacked/lightsim
 */
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const exe = process.argv[2];
const DEBUG_PORT = 9333;
const netLog = join(mkdtempSync(join(tmpdir(), "lightsim-smoke-")), "netlog.json");

if (!exe || !existsSync(exe)) {
  console.error(`✗ no packaged app at ${exe ?? "(no path given)"}`);
  process.exit(1);
}

let log = "";
const child = spawn(exe, [
  "--no-sandbox",
  `--remote-debugging-port=${DEBUG_PORT}`,
  "--remote-allow-origins=*",
  `--log-net-log=${netLog}`,
  "--net-log-capture-mode=Everything",
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

/**
 * The hosts the app contacted, from the net log. The file may be cut short
 * when the app is stopped, so read it as text rather than as JSON.
 */
function contactedHosts(file) {
  let text = "";
  try { text = readFileSync(file, "utf8"); } catch { return null; }
  const hosts = new Set();
  for (const m of text.matchAll(/\b(?:https?|wss?):\/\/(\[[^\]]+\]|[^/:;"\\\s]+)/gi)) hosts.add(m[1].toLowerCase());
  for (const m of text.matchAll(/"host":"([^"]+)"/g)) {
    const h = m[1].replace(/^[a-z]+:\/\//i, "").replace(/[;/].*$/, "").replace(/:\d+$/, "");
    if (h) hosts.add(h.toLowerCase());
  }
  return [...hosts];
}

const LOCAL = new Set(["127.0.0.1", "localhost", "[::1]", "::1"]);

/** Evaluate an expression in the page over the devtools protocol; with
 *  `userGesture` it runs as if the user had clicked (a pop-up needs that). */
function evaluate(page, expression, userGesture = false) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(page.webSocketDebuggerUrl);
    const timer = setTimeout(() => reject(new Error("devtools evaluate timed out")), 30_000);
    ws.onopen = () => ws.send(JSON.stringify({
      id: 1,
      method: "Runtime.evaluate",
      params: { expression, returnByValue: true, userGesture },
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

  // The help pages ship in the UI bundle and the engine serves them (Help →
  // Documentation opens them in the browser).
  const help = await fetch(new URL("/help/index.html", page.url));
  if (!help.ok) fail(`the help pages are missing: /help/index.html answered ${help.status}`);
  console.log("✓ help pages served");

  // A double-click on a part opens its parameters in a window of their own:
  // the one window the shell lets the page open (desktop/src/windows.js).
  const clicked = await evaluate(page, `(() => {
    const nodes = [...document.querySelectorAll('.react-flow__node')];
    const node = nodes.find((n) => /Motor/.test(n.textContent)) ?? nodes[0];
    node?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    return node?.textContent ?? null;
  })()`, true);
  let paramWindow = null;
  for (let i = 0; i < 20 && !paramWindow; i++) {
    await sleep(500);
    const targets = await (await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/list`)).json();
    paramWindow = targets.find((t) => t.type === "page" && t.id !== page.id);
  }
  if (!paramWindow) fail(`a double-click on '${clicked}' opened no parameter window`);
  const form = JSON.parse(await evaluate(paramWindow, `JSON.stringify({
    title: document.title,
    name: document.querySelector('input[aria-label="Name"]')?.value ?? null,
  })`));
  if (!form.name || form.title !== form.name) {
    fail(`the parameter window shows no part's form: ${JSON.stringify(form)}`);
  }
  console.log(`✓ parameter window opened for '${form.title}'`);

  // Nothing beyond this computer, though the app has been open for a while
  // and (on a fresh install) is showing its update question.
  stop();
  await sleep(2000);
  const hosts = contactedHosts(netLog);
  if (hosts === null) {
    console.error(`✗ no net log was written to ${netLog}`);
    process.exit(1);
  }
  const outside = hosts.filter((h) => !LOCAL.has(h));
  if (outside.length) {
    console.error(`✗ the app contacted hosts outside this computer: ${outside.join(", ")}`);
    process.exit(1);
  }
  console.log(`✓ no outside network requests (hosts contacted: ${hosts.join(", ") || "none"})`);

  console.log("\npackaged app smoke test passed");
  process.exit(0);
} catch (err) {
  fail(err.message ?? String(err));
}
