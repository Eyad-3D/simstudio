"use strict";

// node --test (desktop/): the shell's side of .lightsim files (PLT-33,
// src/project-files.js).

const assert = require("node:assert/strict");
const http = require("node:http");
const path = require("node:path");
const { test } = require("node:test");
const { EngineFiles, isProjectFile, projectFilesIn, suggestedName, withSuffix } = require("../src/project-files");

test("only .lightsim files count as project files", () => {
  assert.equal(isProjectFile("/a/car.lightsim"), true);
  assert.equal(isProjectFile("C:\\Teams\\Car.LIGHTSIM"), true);
  assert.equal(isProjectFile("/a/car.json"), false);
  assert.equal(isProjectFile(undefined), false);
  assert.equal(withSuffix("/a/car"), "/a/car.lightsim");
  assert.equal(withSuffix("/a/car.lightsim"), "/a/car.lightsim");
});

test("files named on the command line are found, options and the app skipped", () => {
  const argv = ["--no-sandbox", "/opt/LightSim/lightsim", "models/car.lightsim", "/abs/b.lightsim", "notes.txt"];
  assert.deepEqual(projectFilesIn(argv, "/home/me"), [
    path.resolve("/home/me", "models/car.lightsim"),
    path.resolve("/abs/b.lightsim"),
  ]);
});

test("a project name becomes a safe file name", () => {
  assert.equal(suggestedName("FS car: 2026/v2?"), "FS car_ 2026_v2_.lightsim");
  assert.equal(suggestedName(""), "project.lightsim");
  assert.equal(suggestedName("trailing. "), "trailing.lightsim");
});

test("the engine is called with both secrets, and its refusals become errors", async () => {
  const seen = [];
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      seen.push({ method: req.method, url: req.url, auth: req.headers.authorization, shell: req.headers["x-lightsim-shell"], body });
      res.setHeader("Content-Type", "application/json");
      if (req.url === "/api/files/open" && JSON.parse(body).path.endsWith("bad.lightsim")) {
        res.statusCode = 400;
        res.end(JSON.stringify({ detail: "Not a LightSim project" }));
      } else if (req.url === "/api/files") {
        res.end(JSON.stringify([{ id: "car", path: "/a/car.lightsim", exists: true }]));
      } else {
        res.end(JSON.stringify({ id: "car", path: JSON.parse(body).path, name: "Car" }));
      }
    });
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  try {
    const files = new EngineFiles(`http://127.0.0.1:${server.address().port}`, "launch", "shell");
    assert.deepEqual(await files.open("/a/car.lightsim"), { id: "car", path: "/a/car.lightsim", name: "Car" });
    await files.saveAs("/a/new.lightsim", "p1");
    assert.equal((await files.recent()).length, 1);
    await assert.rejects(files.open("/a/bad.lightsim"), /Not a LightSim project/);
    assert.deepEqual(seen[1], {
      method: "POST", url: "/api/files/save-as", auth: "Bearer launch", shell: "shell",
      body: JSON.stringify({ path: "/a/new.lightsim", projectId: "p1" }),
    });
  } finally {
    server.close();
  }
});
