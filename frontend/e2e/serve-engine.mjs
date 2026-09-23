#!/usr/bin/env node
/**
 * Start the real Python engine for the browser tests, serving the built UI.
 *
 * Playwright's webServer runs this (see playwright.config.ts). It builds
 * nothing: run `npm run build` first. The engine gets a throw-away projects
 * folder seeded with the bundled examples (the ones git tracks, not projects a
 * developer saved into backend/projects), so tests can save freely without
 * touching backend/projects, and the folder is removed when the engine stops.
 * Where the launcher is killed outright (Windows), the next run removes it.
 *
 *   node e2e/serve-engine.mjs --port 8916
 *
 * SIMSTUDIO_PYTHON picks the interpreter (default: python3, or python on
 * Windows); it needs backend/requirements.txt installed.
 */
import { execFileSync, spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const backend = join(root, "backend");
const dist = join(root, "frontend", "dist");

const portArg = process.argv.indexOf("--port");
const port = portArg > 0 ? Number(process.argv[portArg + 1]) : NaN;
if (!Number.isInteger(port) || port <= 0) {
  console.error("usage: node e2e/serve-engine.mjs --port <port>");
  process.exit(2);
}
if (!existsSync(join(dist, "index.html"))) {
  console.error(`✗ no built UI at ${dist} — run \`npm run build\` in frontend/ first`);
  process.exit(1);
}

/** The example projects: the files git tracks in backend/projects. In
 *  development the engine saves there too, and a developer's own projects
 *  would change what the tests see (or give a menu two matching entries). */
function exampleProjects(dir) {
  let names;
  try {
    const out = execFileSync("git", ["ls-files", "-z", "--", "*.json"], { cwd: dir, encoding: "utf8" });
    names = out.split("\0").filter((name) => name && !name.includes("/"));
  } catch {
    names = ["bev-car.json", "hybrid-car.json"]; // no git: the examples the tests use
    console.warn(`git not available; seeding only ${names.join(", ")}`);
  }
  return names.filter((name) => existsSync(join(dir, name)));
}

/** Folders left by launchers that were killed before they could clean up
 *  (on Windows Playwright stops the web server with a hard kill). */
function removeStaleFolders() {
  for (const name of readdirSync(tmpdir())) {
    const pid = Number(/^simstudio-e2e-(\d+)-/.exec(name)?.[1]);
    if (!pid || pid === process.pid) continue;
    try {
      process.kill(pid, 0); // still running: another test run's folder
    } catch (e) {
      if (e.code === "ESRCH") rmSync(join(tmpdir(), name), { recursive: true, force: true });
    }
  }
}

removeStaleFolders();
const projects = mkdtempSync(join(tmpdir(), `simstudio-e2e-${process.pid}-`));
for (const name of exampleProjects(join(backend, "projects"))) {
  copyFileSync(join(backend, "projects", name), join(projects, name));
}
// the engine's first-run seeding (app/storage.py) would otherwise copy in
// every JSON file in backend/projects
writeFileSync(join(projects, ".seeded"), "seeded by e2e/serve-engine.mjs\n");

const python = process.env.SIMSTUDIO_PYTHON || (process.platform === "win32" ? "python" : "python3");
// Same event loop, HTTP parser and WebSocket stack as the packaged engine
// (app/server.py), so the tests see what the desktop app ships.
const child = spawn(
  python,
  [
    "-m", "uvicorn", "app.main:app",
    "--host", "127.0.0.1", "--port", String(port),
    "--loop", "asyncio", "--http", "h11", "--ws", "websockets",
    "--log-level", "warning",
  ],
  {
    cwd: backend,
    stdio: "inherit",
    env: { ...process.env, SIMSTUDIO_PROJECTS_DIR: projects, SIMSTUDIO_STATIC_DIR: dist },
  },
);
console.log(`engine pid ${child.pid} on http://127.0.0.1:${port} (projects in ${projects})`);

const cleanUp = () => rmSync(projects, { recursive: true, force: true });
const stop = () => {
  if (child.exitCode === null) child.kill();
};
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, stop);
child.on("error", (e) => {
  console.error(`✗ could not start ${python}: ${e.message}`);
  cleanUp();
  process.exit(1);
});
child.on("exit", (code, signal) => {
  cleanUp();
  process.exit(code ?? (signal ? 0 : 1));
});
