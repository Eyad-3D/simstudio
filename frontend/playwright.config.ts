// Browser (end-to-end) tests: the built UI against the real Python engine.
//
//   npm run build && npm run test:e2e
//
// The web server below starts the engine only (e2e/serve-engine.mjs); it does
// not build anything, so a stale dist/ tests stale code. The engine listens on
// a free port unless SIMSTUDIO_E2E_PORT pins one.
import { defineConfig, devices } from "@playwright/test";
import { execFileSync } from "node:child_process";

/** Ask the OS for a free loopback port (synchronously: the config is not async). */
function freePort(): number {
  const script =
    "const s=require('net').createServer();" +
    "s.listen(0,'127.0.0.1',()=>{process.stdout.write(String(s.address().port));s.close()})";
  return Number(execFileSync(process.execPath, ["-e", script]).toString());
}

// Workers re-load this file; exporting the port through the environment keeps
// them on the engine the main process started.
process.env.SIMSTUDIO_E2E_PORT ||= String(freePort());
const port = Number(process.env.SIMSTUDIO_E2E_PORT);
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "e2e",
  // a simulation run takes a few seconds; the whole flow stays well under this
  timeout: 90_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  // One at a time: every test shares one engine and its projects folder, and
  // runs are stored on disk per project (RES-02), so a test running beside
  // another saw that test's runs appear and vanish ("1 stored run" not found).
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL,
    acceptDownloads: true,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "chromium",
      // the size the UI audits used; the desktop window opens at 1600×1000
      use: { ...devices["Desktop Chrome"], viewport: { width: 1600, height: 900 } },
    },
  ],
  webServer: {
    command: `node e2e/serve-engine.mjs --port ${port}`,
    url: `${baseURL}/api/health`,
    reuseExistingServer: false,
    timeout: 60_000,
    stdout: "pipe",
    stderr: "pipe",
    // SIGTERM lets the launcher stop the engine and delete its temp projects
    gracefulShutdown: { signal: "SIGTERM", timeout: 5_000 },
  },
});
