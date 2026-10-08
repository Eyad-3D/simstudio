"use strict";

/**
 * The machine-wide policy file (PLT-36).
 *
 * Lab admins and company IT put one JSON file on each PC to fix settings for
 * every user of that PC:
 *
 *   Windows  %ProgramData%\LightSim\policy.json
 *   macOS    /Library/Application Support/LightSim/policy.json
 *   Linux    /etc/lightsim/policy.json
 *
 * Only an administrator can write those folders, so a user cannot change the
 * file. On Windows the folder is found from where Windows itself is
 * installed, not from the %ProgramData% variable, which a user can set for
 * their own account to point LightSim at a file they wrote. A setting the file fixes shows as "managed by your organisation" and
 * cannot be changed in the app. A key that is missing leaves that setting to
 * the user; a key with a value LightSim does not know is ignored and logged
 * (never guessed). docs/help/how-to/deploy-for-it.md lists the keys.
 */

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

/** Each key's allowed values, or the check its value must pass. */
const KEYS = {
  // Update checks: never, tell the user only, or offer to install.
  updates: ["off", "notify", "auto"],
  // Not used yet: LightSim has no AI features. Read and kept so a policy
  // written now keeps working when they arrive.
  ai: ["off", "mcp-only", "allowed"],
  aiProviders: "string-list",
  licenceFile: "string",
  // Folders projects are kept in; the first is the projects folder.
  projectsRoots: "string-list",
  // When to ask before a project's scripts run (PLT-35).
  scriptTrust: ["prompt", "always-prompt"],
  // false: the Start page and Open menu offer no examples.
  examples: "boolean",
};

/**
 * The drive Windows is installed on, as the system's loader saw it: from the
 * path every Windows process loads ntdll.dll from (its system folder). Not
 * from %SystemDrive% or %ProgramData%, which a user can set for their own
 * account (HKCU\\Environment). null when it cannot be told (not Windows).
 */
function windowsDrive(report = () => process.report.getReport()) {
  try {
    for (const lib of report().sharedObjects || []) {
      const m = /^(?:\\\\\?\\)?([A-Za-z]):\\[^\\]+\\(?:system32|syswow64)\\ntdll\.dll$/i.exec(String(lib));
      if (m) return `${m[1].toUpperCase()}:`;
    }
  } catch {
    /* no diagnostic report: fall back */
  }
  return null;
}

/**
 * Where the policy file lives on this platform. On Windows: ProgramData on
 * the drive Windows is on (Windows does not support moving ProgramData);
 * only when that drive cannot be told, the ProgramData variable.
 */
function policyPath(platform = process.platform, env = process.env, drive = platform === "win32" ? windowsDrive() : null) {
  if (platform === "win32") {
    const base = drive ? `${drive}\\ProgramData` : env.ProgramData || env.PROGRAMDATA || "C:\\ProgramData";
    return path.win32.join(base, "LightSim", "policy.json");
  }
  if (platform === "darwin") return "/Library/Application Support/LightSim/policy.json";
  return "/etc/lightsim/policy.json";
}

/**
 * Check a parsed policy. Returns the settings it fixes (only valid keys) and
 * a list of problems, each a sentence for the log.
 */
function validatePolicy(raw) {
  const settings = {};
  const problems = [];
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    return { settings, problems: ["the file does not hold a JSON object"] };
  }
  for (const [key, value] of Object.entries(raw)) {
    const rule = KEYS[key];
    if (rule === undefined) {
      problems.push(`unknown key '${key}' ignored`);
      continue;
    }
    let ok;
    if (Array.isArray(rule)) ok = rule.includes(value);
    else if (rule === "boolean") ok = typeof value === "boolean";
    else if (rule === "string") ok = typeof value === "string" && value.trim() !== "";
    else ok = Array.isArray(value) && value.every((v) => typeof v === "string" && v.trim() !== "");
    if (ok) settings[key] = value;
    else {
      const want = Array.isArray(rule) ? rule.map((v) => `'${v}'`).join(", ") : `a ${rule}`;
      problems.push(`'${key}' ignored: ${JSON.stringify(value)} is not ${want}`);
    }
  }
  return { settings, problems };
}

/**
 * Read the policy file. No file means no policy (the usual case); a file that
 * cannot be read or parsed fixes nothing and says why.
 */
function readPolicy(file = policyPath()) {
  let text;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch (err) {
    if (err && err.code === "ENOENT") return { file, found: false, settings: {}, problems: [] };
    return { file, found: true, settings: {}, problems: [`could not be read (${err.message})`] };
  }
  let raw;
  try {
    raw = JSON.parse(text.replace(/^\uFEFF/, "")); // Notepad may add a BOM
  } catch (err) {
    return { file, found: true, settings: {}, problems: [`is not valid JSON (${err.message})`] };
  }
  return { file, found: true, ...validatePolicy(raw) };
}

/**
 * Expand %VAR% (Windows), $VAR / ${VAR} and a leading ~ in a policy path, so
 * one file can point every user at their own folder (%USERNAME%, $USER).
 */
function expandPath(value, env = process.env, home = os.homedir()) {
  let out = value.replace(/%([A-Za-z0-9_]+)%/g, (m, name) => env[name] ?? m);
  out = out.replace(/\$\{([A-Za-z0-9_]+)\}|\$([A-Za-z0-9_]+)/g, (m, a, b) => env[a || b] ?? m);
  if (out === "~" || out.startsWith("~/") || out.startsWith("~\\")) out = home + out.slice(1);
  return out;
}

/**
 * The environment the engine starts with. The packaged app passes on none
 * of the user's own LIGHTSIM_* variables: they are switches for development
 * and the engine's tests (LIGHTSIM_SCRIPT_TRUST=off turns the script check
 * off), and a user must not be able to override the policy with them. The
 * shell then sets the ones the engine needs.
 */
function engineEnv(env, packaged) {
  const out = { ...env };
  if (packaged) {
    for (const key of Object.keys(out)) if (key.toUpperCase().startsWith("LIGHTSIM_")) delete out[key];
  }
  return out;
}

module.exports = { KEYS, policyPath, windowsDrive, validatePolicy, readPolicy, expandPath, engineEnv };
