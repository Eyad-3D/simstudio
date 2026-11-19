// Sync bundled fallback data from the backend's single source of truth.
// Runs automatically before `npm run dev` / `npm run build` (pre-scripts), so
// the component catalog and demo project are never hand-maintained twice.
import { copyFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..");

const pairs = [
  [
    join(repoRoot, "backend", "app", "library", "components.json"),
    join(here, "..", "src", "data", "componentLibrary.json"),
  ],
  [
    join(repoRoot, "backend", "app", "library", "tyres.json"),
    join(here, "..", "src", "data", "tyres.json"),
  ],
  [
    join(repoRoot, "backend", "projects", "bev-car.json"),
    join(here, "..", "src", "data", "demoProject.json"),
  ],
];

for (const [src, dst] of pairs) {
  copyFileSync(src, dst);
  console.log(`synced ${dst} ← ${src}`);
}
