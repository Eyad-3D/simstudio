"use strict";

// node --test (desktop/): the one window the page may open (a part's
// parameter window) and that every other window.open is handled as before.

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");

const { windowOpenAction, paramWindowOptions, onScreen } = require("../src/windows");

const main = fs.readFileSync(path.join(__dirname, "..", "src", "main.js"), "utf8");

test("a blank page under a parameter window's name may open", () => {
  assert.equal(windowOpenAction({ url: "about:blank", frameName: "lightsim-param-el_1" }), "param");
  assert.equal(windowOpenAction({ url: "", frameName: "lightsim-param-el_1" }), "param");
});

test("every other window.open is handled as before: links to the browser, the rest refused", () => {
  // the name alone opens nothing that is not a blank page
  assert.equal(windowOpenAction({ url: "http://127.0.0.1:8123/", frameName: "lightsim-param-x" }), "external");
  assert.equal(windowOpenAction({ url: "https://github.com/Eyad-3D/simstudio", frameName: "" }), "external");
  assert.equal(windowOpenAction({ url: "http://127.0.0.1:8123/help/index.html", frameName: "_blank" }), "external");
  for (const url of ["file:///C:/Windows/win.ini", "javascript:alert(1)", "data:text/html,hi", "lightsim://x", "about:srcdoc"]) {
    assert.equal(windowOpenAction({ url, frameName: "lightsim-param-x" }), "deny", url);
  }
  // a blank page under any other name, or the bare prefix
  for (const frameName of ["", "_blank", "other", "lightsim-param-", undefined]) {
    assert.equal(windowOpenAction({ url: "about:blank", frameName }), "deny", String(frameName));
  }
});

test("a parameter window has the main window's safety and no preload", () => {
  const { webPreferences } = paramWindowOptions("icon.png");
  assert.deepEqual(webPreferences, { contextIsolation: true, nodeIntegration: false, sandbox: true, spellcheck: false });
  assert.equal("preload" in webPreferences, false);
  const mainPrefs = /webPreferences: \{([^}]*)\}/.exec(main)[1];
  for (const pref of ["contextIsolation: true", "nodeIntegration: false", "sandbox: true"]) assert.match(mainPrefs, new RegExp(pref));
});

test("the main window uses the rule, and a parameter window is kept from going anywhere", () => {
  assert.match(main, /windows\.windowOpenAction\(details\)/);
  assert.match(main, /did-create-window", \(child, \{ frameName \}\) => guardParamWindow\(child, frameName\)/);
  const guard = /function guardParamWindow[\s\S]*?\n}\n/.exec(main)[0];
  assert.match(guard, /setWindowOpenHandler/);
  assert.match(guard, /"will-navigate", \(event, url\) => \{\n\s*event\.preventDefault\(\);/);
  assert.match(guard, /removeMenu\(\)/);
  // raising a window answers only the app's own page
  assert.match(main, /"lightsim:raise-window", \(event, name\) => \{\n\s*if \(!fromApp\(event\)\) return;/);
});

const screenA = { x: 0, y: 0, width: 1920, height: 1040 };
const screenB = { x: 1920, y: 0, width: 2560, height: 1400 };

test("a window on a screen stays where it was", () => {
  assert.equal(onScreen({ x: 2200, y: 100, width: 460, height: 640 }, [screenA, screenB], screenA), null);
  // half off the edge, its title bar still within reach
  assert.equal(onScreen({ x: 1700, y: 900, width: 460, height: 640 }, [screenA], screenA), null);
});

test("a window remembered on a monitor that has gone comes back onto a screen", () => {
  assert.deepEqual(onScreen({ x: 2200, y: 100, width: 460, height: 640 }, [screenA], screenA), {
    x: 730, y: 200, width: 460, height: 640,
  });
  // title bar above the top of every screen
  assert.notEqual(onScreen({ x: 100, y: -500, width: 460, height: 640 }, [screenA], screenA), null);
  // no larger than the screen it lands on
  assert.deepEqual(onScreen({ x: 5000, y: 0, width: 2500, height: 1300 }, [screenA], screenA), {
    x: 0, y: 0, width: 1920, height: 1040,
  });
});
