"use strict";

// node --test (desktop/): the machine-wide policy file (src/policy.js, PLT-36).

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { test } = require("node:test");
const { policyPath, windowsDrive, validatePolicy, readPolicy, expandPath, engineEnv } = require("../src/policy");

test("the policy file lives in a folder only administrators can write", () => {
  assert.equal(policyPath("win32", {}, "C:"), "C:\\ProgramData\\LightSim\\policy.json");
  assert.equal(policyPath("win32", {}, "D:"), "D:\\ProgramData\\LightSim\\policy.json");
  assert.equal(policyPath("linux", {}), "/etc/lightsim/policy.json");
  assert.equal(policyPath("darwin", {}), "/Library/Application Support/LightSim/policy.json");
});

test("a user's own ProgramData variable does not move the policy file", () => {
  // a standard user can set ProgramData in HKCU\\Environment for their account
  const env = { ProgramData: "C:\\Users\\me\\fake", PROGRAMDATA: "C:\\Users\\me\\fake", SystemDrive: "Z:" };
  assert.equal(policyPath("win32", env, "C:"), "C:\\ProgramData\\LightSim\\policy.json");
  // the drive comes from where Windows loaded its own ntdll.dll
  const report = (libs) => () => ({ sharedObjects: libs });
  assert.equal(windowsDrive(report(["C:\\Program Files\\LightSim\\LightSim.exe", "D:\\WINDOWS\\SYSTEM32\\ntdll.dll"])), "D:");
  assert.equal(windowsDrive(report(["\\\\?\\e:\\Windows\\System32\\ntdll.dll"])), "E:");
  assert.equal(windowsDrive(report(["C:\\Users\\me\\system32\\x\\ntdll.dll"])), null);
  assert.equal(windowsDrive(report(["/usr/lib/libc.so.6"])), null);
  assert.equal(windowsDrive(() => { throw new Error("no report"); }), null);
  // only when the drive cannot be told: the variable, then the usual place
  assert.equal(policyPath("win32", { ProgramData: "D:\\ProgramData" }, null), "D:\\ProgramData\\LightSim\\policy.json");
  assert.equal(policyPath("win32", {}, null), "C:\\ProgramData\\LightSim\\policy.json");
});

test("every documented key is accepted with its allowed values", () => {
  const raw = {
    updates: "off",
    ai: "mcp-only",
    aiProviders: ["local"],
    licenceFile: "C:\\Licences\\lightsim.lic",
    projectsRoots: ["H:\\LightSim"],
    scriptTrust: "always-prompt",
    examples: false,
  };
  const { settings, problems } = validatePolicy(raw);
  assert.deepEqual(settings, raw);
  assert.deepEqual(problems, []);
});

test("unknown keys and wrong values are ignored, never guessed", () => {
  const { settings, problems } = validatePolicy({
    updates: "sometimes",
    examples: "no",
    projectsRoots: "H:\\LightSim",
    scriptTrust: "prompt",
    colour: "blue",
  });
  assert.deepEqual(settings, { scriptTrust: "prompt" });
  assert.equal(problems.length, 4);
  assert.match(problems.join("\n"), /unknown key 'colour'/);
  assert.match(problems.join("\n"), /'updates' ignored: "sometimes" is not 'off', 'notify', 'auto'/);
  assert.deepEqual(validatePolicy([1, 2]).settings, {});
  assert.deepEqual(validatePolicy(null).problems, ["the file does not hold a JSON object"]);
});

test("a missing file fixes nothing; a broken one says why", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lightsim-policy-"));
  try {
    assert.deepEqual(readPolicy(path.join(dir, "policy.json")), {
      file: path.join(dir, "policy.json"), found: false, settings: {}, problems: [],
    });
    const file = path.join(dir, "policy.json");
    fs.writeFileSync(file, "{ updates: off }");
    const broken = readPolicy(file);
    assert.equal(broken.found, true);
    assert.deepEqual(broken.settings, {});
    assert.match(broken.problems[0], /is not valid JSON/);
    // Notepad's byte-order mark is fine
    fs.writeFileSync(file, "\uFEFF" + JSON.stringify({ updates: "notify" }));
    assert.deepEqual(readPolicy(file).settings, { updates: "notify" });
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("project folders can name each user's own folder", () => {
  const env = { USERNAME: "ada", USER: "ada", HOMESHARE: "\\\\server\\home" };
  assert.equal(expandPath("H:\\Projects\\%USERNAME%", env), "H:\\Projects\\ada");
  assert.equal(expandPath("%HOMESHARE%\\LightSim", env), "\\\\server\\home\\LightSim");
  assert.equal(expandPath("/srv/lab/$USER/lightsim", env), "/srv/lab/ada/lightsim");
  assert.equal(expandPath("/srv/${USER}", env), "/srv/ada");
  assert.equal(expandPath("~/LightSim", env, "/home/ada"), "/home/ada/LightSim");
  // an unknown variable stays as written, so the mistake is visible
  assert.equal(expandPath("%NOPE%\\x", env), "%NOPE%\\x");
});

test("the installed app passes none of the user's LIGHTSIM_ switches to its engine", () => {
  const env = { PATH: "/bin", LIGHTSIM_SCRIPT_TRUST: "off", lightsim_policy: "{}", LIGHTSIMX: "1" };
  assert.deepEqual(engineEnv(env, true), { PATH: "/bin", LIGHTSIMX: "1" });
  assert.deepEqual(engineEnv(env, false), env); // running from source: a developer's switches work
  assert.equal(env.LIGHTSIM_SCRIPT_TRUST, "off"); // not changed in place
});
