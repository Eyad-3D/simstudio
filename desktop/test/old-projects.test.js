"use strict";

// node --test (desktop/): the copy of SimStudio's projects into LightSim's
// data folder on first launch (src/old-projects.js).

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { test, beforeEach, afterEach } = require("node:test");
const { copyOldProjects } = require("../src/old-projects");

let appData, userData, oldProjects, newProjects, logged;
const log = (message) => logged.push(message);

/** Every file under `dir` with its contents, keyed by its relative path. */
function tree(dir) {
  const out = {};
  for (const entry of fs.readdirSync(dir, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const file = path.join(entry.parentPath, entry.name);
    out[path.relative(dir, file).split(path.sep).join("/")] = fs.readFileSync(file, "utf8");
  }
  return out;
}

function write(file, text) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
}

beforeEach(() => {
  appData = fs.mkdtempSync(path.join(os.tmpdir(), "lightsim-appdata-"));
  userData = path.join(appData, "LightSim");
  oldProjects = path.join(appData, "SimStudio", "projects");
  newProjects = path.join(userData, "projects");
  logged = [];
});

afterEach(() => fs.rmSync(appData, { recursive: true, force: true }));

function oldInstall() {
  write(path.join(oldProjects, "my-car.json"), '{"id":"my-car"}');
  write(path.join(oldProjects, "my-car.json.bak"), '{"id":"my-car","v":1}');
  write(path.join(oldProjects, ".hidden-examples"), '["bev-car"]');
  write(path.join(oldProjects, ".backups", "my-car", "1767600000000-0123456789abcdef.json"), "{}");
  write(path.join(oldProjects, "runs", "my-car", "index.json"), '{"runs":[]}');
  write(path.join(appData, "SimStudio", "backend-port.json"), '{"port":51234}');
  return tree(oldProjects);
}

test("copies the old projects, runs, backups and hidden examples on first launch", async () => {
  const before = oldInstall();
  assert.equal(await copyOldProjects(appData, userData, log), "copied");
  assert.deepEqual(tree(newProjects), before);
  assert.deepEqual(tree(oldProjects), before, "SimStudio's folder is left as it was");
  assert.equal(fs.existsSync(`${newProjects}.copying`), false);
  assert.match(logged.join("\n"), /Copied the projects saved by SimStudio/);
  // only the projects folder: nothing else of SimStudio's comes along
  assert.deepEqual(fs.readdirSync(userData), ["projects"]);
});

test("keeps the file times, which the run history is ordered by", async () => {
  oldInstall();
  const file = path.join(oldProjects, "runs", "my-car", "index.json");
  const when = new Date("2026-03-01T12:00:00Z");
  fs.utimesSync(file, when, when);
  await copyOldProjects(appData, userData, log);
  const copied = fs.statSync(path.join(newProjects, "runs", "my-car", "index.json"));
  assert.equal(copied.mtime.getTime(), when.getTime());
});

test("does nothing when LightSim already has projects", async () => {
  oldInstall();
  write(path.join(newProjects, "new-car.json"), '{"id":"new-car"}');
  assert.equal(await copyOldProjects(appData, userData, log), "already has projects");
  assert.deepEqual(tree(newProjects), { "new-car.json": '{"id":"new-car"}' });
  assert.deepEqual(logged, []);
});

test("happens once: a second launch changes nothing", async () => {
  oldInstall();
  await copyOldProjects(appData, userData, log);
  write(path.join(oldProjects, "later.json"), "{}"); // saved by SimStudio afterwards
  assert.equal(await copyOldProjects(appData, userData, log), "already has projects");
  assert.equal(fs.existsSync(path.join(newProjects, "later.json")), false);
});

test("does nothing without an old projects folder, or with an empty one", async () => {
  assert.equal(await copyOldProjects(appData, userData, log), "nothing to copy");
  fs.mkdirSync(oldProjects, { recursive: true });
  assert.equal(await copyOldProjects(appData, userData, log), "nothing to copy");
  assert.equal(fs.existsSync(newProjects), false);
});

test("fills an empty projects folder left by an earlier launch", async () => {
  const before = oldInstall();
  fs.mkdirSync(newProjects, { recursive: true });
  assert.equal(await copyOldProjects(appData, userData, log), "copied");
  assert.deepEqual(tree(newProjects), before);
});

test("retries a copy that was cut short", async () => {
  const before = oldInstall();
  write(path.join(`${newProjects}.copying`, "my-car.json"), "{half");
  assert.equal(await copyOldProjects(appData, userData, log), "copied");
  assert.deepEqual(tree(newProjects), before);
  assert.equal(fs.existsSync(`${newProjects}.copying`), false);
});

test("a failed copy logs why, leaves no partial folder and is tried again next launch", async (t) => {
  const before = oldInstall();
  t.mock.method(fs.promises, "cp", async () => {
    throw new Error("disk full");
  });
  assert.equal(await copyOldProjects(appData, userData, log), "failed");
  assert.match(logged.join("\n"), /Could not copy .*disk full/);
  assert.equal(fs.existsSync(newProjects), false);
  assert.equal(fs.existsSync(`${newProjects}.copying`), false);
  assert.deepEqual(tree(oldProjects), before);

  t.mock.restoreAll();
  assert.equal(await copyOldProjects(appData, userData, log), "copied");
  assert.deepEqual(tree(newProjects), before);
});
