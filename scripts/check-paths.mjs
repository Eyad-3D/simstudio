#!/usr/bin/env node
/**
 * Every file in the repository must be one Windows can check out: LightSim
 * ships for Windows only, and a name Windows refuses stops every Windows
 * job at its first step (git: "invalid path"). Windows refuses the device
 * names (CON, PRN, AUX, NUL, COM1-9, LPT1-9, with or without an extension),
 * the characters < > : " | ? * \ and control characters, and names ending
 * in a dot or a space; two paths that differ only in letter case would
 * also overwrite each other.
 *
 *   node scripts/check-paths.mjs
 */
import { execFileSync } from "node:child_process";

const files = execFileSync("git", ["ls-files", "-z"], { encoding: "utf8" }).split("\0").filter(Boolean);
const device = /^(con|prn|aux|nul|com[0-9¹²³]|lpt[0-9¹²³])(\..*)?$/i;
const bad = [];
const seen = new Map();
for (const f of files) {
  for (const part of f.split("/")) {
    // eslint-disable-next-line no-control-regex
    if (device.test(part) || /[<>:"|?*\\\x00-\x1f]/.test(part) || /[. ]$/.test(part)) {
      bad.push(`${f}: "${part}" is not a valid name on Windows`);
    }
  }
  const key = f.toLowerCase();
  if (seen.has(key)) bad.push(`${f} and ${seen.get(key)} differ only in letter case`);
  else seen.set(key, f);
}
if (bad.length) {
  console.error(bad.join("\n"));
  process.exit(1);
}
console.log(`${files.length} paths: every one valid on Windows`);
