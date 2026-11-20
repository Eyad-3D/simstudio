"use strict";

// node --test (desktop/): the windows the page may open (a part's parameter
// window, the Help window) and that every other window.open is handled as before.

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");

const { windowOpenAction, isHelpUrl, paramWindowOptions, onScreen } = require("../src/windows");

// (a Windows checkout has CRLF line ends: the patterns below are written for LF)
const main = fs.readFileSync(path.join(__dirname, "..", "src", "main.js"), "utf8").replace(/\r\n/g, "\n");

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

const ORIGIN = "http://127.0.0.1:8123";
const help = (url, from = "main", frameName = "lightsim-help") => windowOpenAction({ url, frameName }, ORIGIN, from);

test("the Help window may open at the app's own help pages, from the main window or a parameter window", () => {
  for (const from of ["main", "param"]) {
    assert.equal(help(`${ORIGIN}/help/index.html`, from), "help", from);
    assert.equal(help(`${ORIGIN}/help/reference/components/propulsion.e_motor.html#max_torque`, from), "help", from);
  }
  assert.equal(isHelpUrl(`${ORIGIN}/help/glossary.html#soc`, ORIGIN), true);
});

test("the Help window opens nowhere else", () => {
  // another path on the app's origin, also by "..", by %2e%2e or by a look-alike folder
  for (const url of [`${ORIGIN}/`, `${ORIGIN}/api/projects`, `${ORIGIN}/help`, `${ORIGIN}/helpers/x.html`,
    `${ORIGIN}/help/../api/projects`, `${ORIGIN}/help/%2e%2e/index.html`, `${ORIGIN}/HELP/index.html`]) {
    assert.equal(isHelpUrl(url, ORIGIN), false, url);
    assert.equal(help(url), "external", url); // an http(s) link, as before: to the browser
  }
  // another origin: another port, host or scheme, or a user name in front
  for (const url of ["http://127.0.0.1:9999/help/index.html", "http://localhost:8123/help/index.html",
    "https://127.0.0.1:8123/help/index.html", "http://evil@127.0.0.1:8123/help/index.html",
    "https://github.com/Eyad-3D/simstudio/help/index.html"]) {
    assert.equal(help(url), "external", url);
  }
  // no file:, javascript:, data: or blank page under the Help window's name
  for (const url of ["file:///C:/help/index.html", "javascript:alert(1)", "data:text/html,/help/", "about:blank", "",
    "not a url"]) {
    assert.equal(help(url), "deny", url);
  }
  // not before the engine runs, not from the Help window itself, not under another name
  assert.equal(windowOpenAction({ url: `${ORIGIN}/help/index.html`, frameName: "lightsim-help" }), "external");
  assert.equal(help(`${ORIGIN}/help/index.html`, "help"), "external");
  assert.equal(help(`${ORIGIN}/help/index.html`, "main", "lightsim-help-2"), "external");
  // and only the main window opens a parameter window
  assert.equal(windowOpenAction({ url: "about:blank", frameName: "lightsim-param-x" }, ORIGIN, "param"), "deny");
  assert.equal(windowOpenAction({ url: "about:blank", frameName: "lightsim-param-x" }, ORIGIN, "help"), "deny");
});

test("a parameter window has the main window's safety and no preload", () => {
  const { webPreferences } = paramWindowOptions("icon.png");
  assert.deepEqual(webPreferences, { contextIsolation: true, nodeIntegration: false, sandbox: true, spellcheck: false });
  assert.equal("preload" in webPreferences, false);
  const mainPrefs = /webPreferences: \{([^}]*)\}/.exec(main)[1];
  for (const pref of ["contextIsolation: true", "nodeIntegration: false", "sandbox: true"]) assert.match(mainPrefs, new RegExp(pref));
});

test("every window uses the rule, and a child window is kept from going anywhere", () => {
  assert.match(main, /windows\.windowOpenAction\(details, appOrigin, from\)/);
  assert.match(main, /mainWindow\.webContents\.setWindowOpenHandler\(openHandler\("main"\)\)/);
  assert.match(main, /did-create-window", \(child, \{ frameName \}\) => guardParamWindow\(child, frameName\)/);
  // the Help window: the same safety, and it stays when the parameter window it came from closes
  const handler = /function openHandler[\s\S]*?\n}\n/.exec(main)[0];
  assert.match(handler, /action === "help"\) \{\n\s*return \{ action: "allow", outlivesOpener: true, overrideBrowserWindowOptions: windows\.paramWindowOptions\(ICON\) \}/);
  const guard = /function guardParamWindow[\s\S]*?\n}\n/.exec(main)[0];
  assert.match(guard, /setWindowOpenHandler\(openHandler\(help \? "help" : "param"\)\)/);
  assert.match(guard, /did-create-window", \(next, \{ frameName \}\) => guardParamWindow\(next, frameName\)/);
  // a navigation is stopped unless it is the Help window going to a help page
  assert.match(guard, /"will-navigate", \(event, url\) => \{\n\s*if \(help && windows\.isHelpUrl\(url, appOrigin\)\) return;\n\s*event\.preventDefault\(\);/);
  assert.match(guard, /removeMenu\(\)/);
  assert.match(guard, /mainWindow\.once\("closed", \(\) => child\.isDestroyed\(\) \|\| child\.close\(\)\)/);
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
