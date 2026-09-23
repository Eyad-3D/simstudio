#!/usr/bin/env node
// One version number, three places that need it.
//
//   node scripts/sync-version.mjs           # write VERSION into the manifests
//   node scripts/sync-version.mjs --check   # fail if they have drifted (CI)
//
// The root VERSION file is the source of truth. electron-builder and npm both
// insist on reading their own package.json, so those are kept in step here
// rather than being hand-edited and silently diverging.
//
// docs/KNOWN-LIMITS.md names the version it was last reviewed for. That line
// is checked but never written: a new version needs the page reviewed, not
// just relabelled.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const version = readFileSync(join(root, "VERSION"), "utf8").trim();

if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version)) {
  console.error(`VERSION is not a valid semver string: "${version}"`);
  process.exit(1);
}

const manifests = ["frontend/package.json", "desktop/package.json"];
const check = process.argv.includes("--check");
let drifted = 0;

for (const rel of manifests) {
  const path = join(root, rel);
  const raw = readFileSync(path, "utf8");
  const pkg = JSON.parse(raw);
  if (pkg.version === version) continue;
  if (check) {
    console.error(`${rel}: ${pkg.version} != VERSION (${version})`);
    drifted++;
    continue;
  }
  // Change only the top-level "version" line, so escapes such as \u2014
  // elsewhere in the file survive and the diff stays to the one line.
  const updated = raw.replace(/^(  "version": )"[^"]*"/m, `$1"${version}"`);
  if (updated === raw) {
    console.error(`${rel}: no top-level "version" line to update`);
    process.exit(1);
  }
  writeFileSync(path, updated);
  console.log(`${rel} → ${version}`);
}

const limitsRel = "docs/KNOWN-LIMITS.md";
const limits = readFileSync(join(root, limitsRel), "utf8");
// The "Last reviewed" bullet may wrap; stay inside it (continuation lines are
// indented) so a later "for version" elsewhere on the page cannot match.
const reviewed = /Last reviewed:[^\n]*(?:\n +[^\n]*)*?\bfor\s+version\s+(\S+?)\.?(?:\s|$)/.exec(limits)?.[1];
const unreviewed = reviewed !== version;
if (unreviewed) {
  const say = check ? console.error : console.warn;
  say(`${limitsRel}: last reviewed for ${reviewed ?? "no version"}, VERSION is ${version}.` +
      ` Review the page, then update its "Last reviewed" line.`);
}

if (check && drifted) {
  console.error(`\n${drifted} manifest(s) out of sync. Run: node scripts/sync-version.mjs`);
}
if (check && (drifted || unreviewed)) process.exit(1);
if (check) console.log(`all manifests and ${limitsRel} at ${version}`);
