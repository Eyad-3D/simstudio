"use strict";

// node --test (desktop/): what the About box says about validation, and that
// its button opens the page that says it in full.

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");

const about = require("../src/about");

const ROOT = path.join(__dirname, "..", "..");
// a Windows checkout has CRLF line ends
const read = (...parts) => fs.readFileSync(path.join(ROOT, ...parts), "utf8").replace(/\r\n/g, "\n");
const flat = (text) => text.replace(/\s+/g, " ");

test("the About box no longer says that nothing is validated", () => {
  const text = about.detail("C:\\Users\\a\\projects");
  assert.doesNotMatch(text, /nothing is validated against|not validated/i);
  assert.match(text, /only the energy use of four electric cars on EPA's two test cycles/);
});

test("the About box gives the figure the validation page gives", () => {
  const page = flat(read("docs", "VALIDATION-STATUS.md"));
  const said = /LightSim is within (\d+) % on all eight figures/.exec(page);
  assert.ok(said, "the validation page's one line has changed");
  assert.match(about.detail("x"), new RegExp(`within ${said[1]} %`));
});

test("the box keeps the owner's copyright and licence lines as they were", () => {
  const text = about.detail("D:\\LightSim");
  assert.ok(text.includes("Projects folder:\nD:\\LightSim\n\n"));
  assert.ok(text.includes("Copyright © 2026 Eyad Abualkhair. All rights reserved.\n"));
  assert.ok(text.includes("Free for non-commercial use; commercial use needs a licence (see EULA.txt).\n"));
  assert.ok(
    text.endsWith("Includes open-source software and data under their own licences (Help → Third-Party Notices)."),
  );
});

test("the second button opens the validation page, as Help → What Is Validated does", () => {
  assert.deepEqual(about.BUTTONS, ["OK", "What Is Validated"]);
  assert.equal(about.VALIDATION_PAGE, "validation.html");
  const main = read("desktop", "src", "main.js");
  // the Help menu's own item opens the same page
  assert.match(main, /label: "What Is Validated",\s+click: \(\) => helpPage\("validation\.html"\)/);
  // and the box opens it, not Known Limits
  assert.match(main, /if \(response === 1\) helpPage\(about\.VALIDATION_PAGE\);/);
  assert.doesNotMatch(main, /buttons: \["OK", "Known Limits"\]/);
  // the help the app serves builds validation.html from the validation page
  const build = read("frontend", "scripts", "build-docs.mjs");
  assert.match(build, /\["Validation", "validation\.md", "VALIDATION-STATUS\.md"\]/);
});
