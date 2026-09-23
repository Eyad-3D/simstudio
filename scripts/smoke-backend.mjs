#!/usr/bin/env node
/**
 * Smoke-test the FROZEN backend executable.
 *
 * Unit tests run against source Python, so they cannot see what PyInstaller
 * left out of the bundle. This starts the real executable and exercises the
 * paths that depend on the bundle being complete: the component library, the
 * example projects, a REST simulation, and a live run over the WebSocket.
 * (A missing websockets module passed every unit test and would have shipped
 * a broken "Run" button — hence this script.)
 *
 *   node scripts/smoke-backend.mjs [path-to-executable]
 */
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const isWindows = process.platform === "win32";
const exeName = isWindows ? "simstudio-backend.exe" : "simstudio-backend";
const exe = process.argv[2] ?? join(root, "backend", "dist", "simstudio-backend", exeName);
const PORT = 8971;
const base = `http://127.0.0.1:${PORT}`;

if (!existsSync(exe)) {
  console.error(`✗ no frozen backend at ${exe} — build it first`);
  process.exit(1);
}

let log = "";
const child = spawn(exe, ["--port", String(PORT), "--host", "127.0.0.1"], {
  env: { ...process.env, SIMSTUDIO_PROJECTS_DIR: join(root, ".smoke-projects") },
});
child.stdout.on("data", (d) => (log += d));
child.stderr.on("data", (d) => (log += d));

const stop = () => { try { child.kill(); } catch { /* already gone */ } };
const fail = (msg) => {
  console.error(`✗ ${msg}\n\n--- backend output ---\n${log.slice(-3000)}`);
  stop();
  process.exit(1);
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitForHealth(timeoutMs = 90_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) fail(`backend exited early (code ${child.exitCode})`);
    try {
      const res = await fetch(`${base}/api/health`);
      if (res.ok) return res.json();
    } catch { /* not listening yet */ }
    await sleep(500);
  }
  fail(`backend did not become healthy within ${timeoutMs / 1000}s`);
}

async function liveRun(project, caseId) {
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/simulate/run`);
  let steps = 0;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("no 'done' within 120s")), 120_000);
    ws.onopen = () => ws.send(JSON.stringify({ type: "start", project, caseId }));
    ws.onerror = () => { clearTimeout(timer); reject(new Error("websocket failed to connect")); };
    ws.onmessage = (e) => {
      const m = JSON.parse(e.data);
      if (m.type === "step") steps++;
      if (m.type === "error") { clearTimeout(timer); reject(new Error(m.detail)); }
      if (m.type === "done") { clearTimeout(timer); ws.close(); resolve({ steps, result: m.result }); }
    };
  });
}

try {
  const health = await waitForHealth();
  console.log(`✓ health: ${health.status} (version ${health.version})`);

  const lib = await (await fetch(`${base}/api/library`)).json();
  if (!lib.components?.length) fail("component library came back empty");
  console.log(`✓ library: ${lib.components.length} components`);

  const examples = await (await fetch(`${base}/api/examples`)).json();
  if (!examples.length) fail("no example projects in the bundle");
  console.log(`✓ examples: ${examples.map((p) => p.id).join(", ")}`);

  const project = await (await fetch(`${base}/api/examples/${examples[0].id}`)).json();
  const caseId = project.cases?.[0]?.id;
  if (!caseId) fail(`example '${examples[0].id}' has no simulation case`);

  const rest = await (await fetch(`${base}/api/simulate`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ project, caseId }),
  })).json();
  if (rest.status !== "success") fail(`REST simulate returned '${rest.status}'`);
  console.log(`✓ REST simulate: ${rest.channels.length} channels`);

  const live = await liveRun(project, caseId);
  if (live.result.status !== "success") fail(`live run returned '${live.result.status}'`);
  if (live.steps < 1) fail("live run streamed no steps — the WebSocket path is broken");
  console.log(`✓ live run: ${live.steps} streamed steps, ${live.result.channels.length} channels`);

  console.log("\nfrozen backend smoke test passed");
  stop();
  process.exit(0);
} catch (err) {
  fail(err.message ?? String(err));
}
