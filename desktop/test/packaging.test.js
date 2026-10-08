"use strict";

// node --test (desktop/): what the installers carry besides the shell (STD-09).

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");

const ROOT = path.join(__dirname, "..", "..");
const builder = fs.readFileSync(path.join(__dirname, "..", "electron-builder.yml"), "utf8");

/** The extraResources entries as {from, to} (the config's own simple form). */
function extraResources() {
  const block = builder.split(/^extraResources:\n/m)[1].split(/^\S/m)[0];
  return [...block.matchAll(/- from: (\S+)\n\s+to: (\S+)/g)].map((m) => ({ from: m[1], to: m[2] }));
}

test("lightsim_run.m is installed in resources/matlab, beside the engine", () => {
  const entries = extraResources();
  assert.ok(entries.some((e) => e.from === "../matlab" && e.to === "matlab"), JSON.stringify(entries));
  assert.ok(entries.some((e) => e.from === "../backend/dist/lightsim-backend" && e.to === "backend"));
  assert.ok(fs.existsSync(path.join(ROOT, "matlab", "lightsim_run.m")));
});

test("the installed lightsim_run.m finds the engine from its own folder first", () => {
  const m = fs.readFileSync(path.join(ROOT, "matlab", "lightsim_run.m"), "utf8");
  const find = m.split("function engine = find_engine(given)")[1];
  const first = find.indexOf("fullfile(here, '..', 'backend', 'lightsim-backend.exe')");
  assert.ok(find.includes("here = fileparts(mfilename('fullpath'));"));
  assert.ok(first > 0 && first < find.indexOf("LOCALAPPDATA"), "Windows: next to itself before the usual folders");
  const unix = find.indexOf("fullfile(here, '..', 'backend', 'lightsim-backend')");
  assert.ok(unix > 0 && unix < find.indexOf("/opt/LightSim"), "Linux and macOS: next to itself first");
});

test("the help says where lightsim_run.m is installed", () => {
  const help = fs.readFileSync(path.join(ROOT, "docs", "help", "how-to", "use-results-in-matlab-and-python.md"), "utf8");
  assert.match(help, /resources[\\/]matlab/);
});
