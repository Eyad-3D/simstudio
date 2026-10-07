"use strict";

// node --test (desktop/): update checks that ask first (src/updates.js, PLT-18).

const assert = require("node:assert/strict");
const { test } = require("node:test");
const {
  DAY_MS, QUESTION, installKind, canInstall, decideMode, checkDue, resultsThatChange, offerText,
} = require("../src/updates");

test("the install kind decides whether LightSim can update itself", () => {
  const exists = (f) => f === "C:\\Program Files\\LightSim\\Uninstall LightSim.exe";
  const win = { isPackaged: true, platform: "win32", env: {}, exists };
  assert.equal(installKind({ ...win, execPath: "C:\\Program Files\\LightSim\\LightSim.exe" }), "nsis");
  assert.equal(installKind({ ...win, execPath: "C:\\Program Files\\LightSim MSI\\LightSim.exe" }), "msi");
  assert.equal(installKind({ isPackaged: true, platform: "linux", execPath: "/x", env: { APPIMAGE: "/a.AppImage" } }), "appimage");
  assert.equal(installKind({ isPackaged: true, platform: "linux", execPath: "/opt/LightSim/lightsim", env: {} }), "deb");
  assert.equal(installKind({ isPackaged: true, platform: "darwin", execPath: "/x", env: {} }), "mac");
  assert.equal(installKind({ isPackaged: false, platform: "linux", execPath: "/x", env: {} }), "dev");

  assert.equal(canInstall("nsis", { signedWindows: true }), true);
  assert.equal(canInstall("nsis", { signedWindows: false }), false); // could not check the publisher
  assert.equal(canInstall("msi"), false);
  assert.equal(canInstall("deb"), false);
  assert.equal(canInstall("appimage"), true);
  assert.equal(canInstall("mac"), true);
});

test("nothing is checked until the user says yes", () => {
  for (const kind of ["nsis", "appimage", "deb", "mac"]) {
    assert.equal(decideMode({ kind, settings: {} }).mode, "ask");
    assert.equal(decideMode({ kind, settings: { consent: "later" } }).mode, "ask");
    assert.equal(decideMode({ kind, settings: { consent: "no" } }).mode, "off");
  }
  assert.deepEqual(decideMode({ kind: "nsis", settings: { consent: "yes" }, installable: true }),
    { mode: "install", managed: false, checks: true });
  // a .deb or an unsigned build only says a new version exists
  assert.equal(decideMode({ kind: "deb", settings: { consent: "yes" } }).mode, "notify");
  // running from source never checks
  assert.equal(decideMode({ kind: "dev", settings: { consent: "yes" } }).mode, "off");
});

test("MSI installs are off unless the policy turns updates on", () => {
  assert.equal(decideMode({ kind: "msi", settings: {} }).mode, "off");
  assert.equal(decideMode({ kind: "msi", settings: { consent: "yes" } }).mode, "off");
  assert.equal(decideMode({ kind: "msi", policy: { updates: "notify" } }).mode, "notify");
  assert.equal(decideMode({ kind: "msi", policy: { updates: "auto" } }).mode, "notify"); // cannot install
});

test("the policy file overrides the user's answer and locks it", () => {
  const yes = { consent: "yes" };
  assert.deepEqual(decideMode({ kind: "nsis", policy: { updates: "off" }, settings: yes, installable: true }),
    { mode: "off", managed: true, checks: false });
  assert.deepEqual(decideMode({ kind: "nsis", policy: { updates: "notify" }, settings: {}, installable: true }),
    { mode: "notify", managed: true, checks: true });
  assert.deepEqual(decideMode({ kind: "appimage", policy: { updates: "auto" }, settings: { consent: "no" }, installable: true }),
    { mode: "install", managed: true, checks: true });
});

test("at most one scheduled check a day", () => {
  const now = 1_800_000_000_000;
  assert.equal(checkDue({}, now), true);
  assert.equal(checkDue({ lastCheck: now - DAY_MS + 1000 }, now), false);
  assert.equal(checkDue({ lastCheck: now - DAY_MS }, now), true);
  assert.equal(checkDue({ lastCheck: now + DAY_MS }, now), true); // clock moved back
});

test("the question says what is sent and has Yes, No and Ask later", () => {
  assert.equal(QUESTION.message, "Check for updates once a day?");
  assert.deepEqual(QUESTION.buttons, ["Yes", "No", "Ask later"]);
  assert.match(QUESTION.detail, /only its version number and platform/);
  assert.match(QUESTION.detail, /never installs an update without asking/);
});

const MARKDOWN = `## 0.4.0

### Your results will change — here is why

| What changed | Effect on results | Roadmap |
|---|---|---|
| Tyres | BEV WLTC 14.05 → 14.10 kWh/100 km | MOD-50 |

### New

- A thing
`;

const HTML = `<h3>Your results will change — here is why</h3>
<table><thead><tr><th>What changed</th><th>Effect on results</th><th>Roadmap</th></tr></thead>
<tbody><tr><td>Tyres</td><td>BEV WLTC 14.05 &rarr; 14.10 kWh/100&nbsp;km</td><td>MOD-50</td></tr></tbody></table>
<h3>New</h3><ul><li>A thing</li></ul>`;

test("the results that change are shown before installing", () => {
  const md = resultsThatChange(MARKDOWN);
  assert.match(md, /Tyres \| BEV WLTC 14\.05 → 14\.10 kWh\/100 km \| MOD-50/);
  assert.doesNotMatch(md, /A thing/);
  assert.doesNotMatch(md, /---/);

  const html = resultsThatChange(HTML);
  assert.match(html, /Tyres \| BEV WLTC 14\.05 → 14\.10 kWh\/100 km \| MOD-50/);
  assert.doesNotMatch(html, /A thing/);

  // several versions between this one and the new one: each its own part
  const both = resultsThatChange([{ version: "0.5.0", note: HTML }, { version: "0.4.0", note: MARKDOWN }]);
  assert.match(both, /^0\.5\.0:\n/);
  assert.match(both, /\n\n0\.4\.0:\n/);

  assert.equal(resultsThatChange("<p>Bug fixes</p>"), null);
  assert.equal(resultsThatChange(null), null);
  assert.ok(resultsThatChange(MARKDOWN.replace("Tyres", "x".repeat(5000))).endsWith("…"));
});

test("the offer never installs silently", () => {
  const install = offerText({ version: "0.4.0", mode: "install", releaseNotes: MARKDOWN });
  assert.deepEqual(install.buttons, ["Install on quit", "Release notes", "Skip this version", "Later"]);
  assert.match(install.detail, /What changes in your results/);
  const notify = offerText({ version: "0.4.0", mode: "notify", releaseNotes: "" });
  assert.deepEqual(notify.buttons, ["Download page", "Release notes", "Skip this version", "Later"]);
  assert.match(notify.detail, /no changes to results/);
});
