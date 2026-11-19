"use strict";

// node --test (desktop/): when the Windows build is signed (build/sign-windows.cjs, PLT-32).

const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const path = require("node:path");
const { test } = require("node:test");
const { route, missingAzure } = require("../build/sign-windows.cjs");

const AZURE = {
  AZURE_TENANT_ID: "t", AZURE_CLIENT_ID: "c", AZURE_CLIENT_SECRET: "s",
  LIGHTSIM_AZURE_ENDPOINT: "https://weu.codesigning.azure.net", LIGHTSIM_AZURE_ACCOUNT: "a", LIGHTSIM_AZURE_PROFILE: "p",
};

test("signing starts only when all of a route's settings, secrets included, are set", () => {
  assert.equal(route(AZURE), "azure");
  assert.equal(route({ LIGHTSIM_SIGN_COMMAND: "signtool sign \"{file}\"" }), "command");
  assert.equal(route({}), null);
  // the repository variables alone (no secrets yet, or one removed): unsigned
  const vars = { LIGHTSIM_AZURE_ENDPOINT: "e", LIGHTSIM_AZURE_ACCOUNT: "a", LIGHTSIM_AZURE_PROFILE: "p" };
  assert.equal(route(vars), null);
  assert.deepEqual(missingAzure(vars), ["AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET"]);
  for (const key of Object.keys(AZURE)) {
    assert.equal(route({ ...AZURE, [key]: "" }), null, key);
  }
  assert.deepEqual(missingAzure({}), []);
});

test("the workflow asks the same question from the command line", () => {
  const script = path.join(__dirname, "..", "build", "sign-windows.cjs");
  const ask = (env) => execFileSync(process.execPath, [script, "--route"], { env, encoding: "utf8" }).trim();
  assert.equal(ask({ PATH: process.env.PATH }), "none");
  assert.equal(ask({ PATH: process.env.PATH, ...AZURE }), "azure");
  assert.equal(ask({ PATH: process.env.PATH, LIGHTSIM_AZURE_ENDPOINT: "e", LIGHTSIM_AZURE_ACCOUNT: "a", LIGHTSIM_AZURE_PROFILE: "p" }), "none");
});
