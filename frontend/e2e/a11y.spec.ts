// Accessibility gate (axe-core) on three screens: the topology workspace, the
// Results page and a parameter dialog. It fails on any serious or critical
// violation that is not in a11y-baseline.json, the list of issues the app had
// when the gate was introduced. Fixing one of those is reported (so the
// baseline can shrink) but never fails the test.
//
// Regenerate the baseline after a deliberate change:
//   UPDATE_A11Y_BASELINE=1 npx playwright test a11y
import AxeBuilder from "@axe-core/playwright";
import { readFileSync, writeFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { openApp, runActiveCase } from "./app";

type Baseline = Record<string, string[]>;
const BASELINE_FILE = new URL("./a11y-baseline.json", import.meta.url);
const baseline: Baseline = JSON.parse(readFileSync(BASELINE_FILE, "utf8"));
const updating = Boolean(process.env.UPDATE_A11Y_BASELINE);
const found: Baseline = {};

/** Serious/critical violations on the page as "rule-id  target" strings. */
async function blockingViolations(page: Page): Promise<string[]> {
  const { violations } = await new AxeBuilder({ page }).analyze();
  const keys = violations
    .filter((v) => v.impact === "serious" || v.impact === "critical")
    .flatMap((v) => v.nodes.map((n) => `${v.id}  ${n.target.join(" ")}`));
  return [...new Set(keys)].sort();
}

async function check(page: Page, screen: string) {
  const current = await blockingViolations(page);
  if (updating) {
    found[screen] = current;
    return;
  }
  const known = new Set(baseline[screen] ?? []);
  const fixed = [...known].filter((k) => !current.includes(k));
  if (fixed.length) {
    test.info().annotations.push({
      type: "a11y-fixed",
      description: `no longer found on ${screen}, remove from a11y-baseline.json: ${fixed.join(" | ")}`,
    });
  }
  const added = current.filter((k) => !known.has(k));
  expect(added, `new serious/critical axe violations on the ${screen} screen`).toEqual([]);
}

test.afterAll(() => {
  if (updating) writeFileSync(BASELINE_FILE, JSON.stringify({ ...baseline, ...found }, null, 2) + "\n");
});

test("topology workspace", async ({ page }) => {
  await openApp(page);
  await check(page, "topology");
});

test("results page", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  await expect(page.locator(".recharts-line-curve").first()).toBeVisible();
  await check(page, "results");
});

test("parameter dialog", async ({ page }) => {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().dblclick();
  await expect(page.getByTitle("Close (Esc)")).toBeVisible();
  await check(page, "parameter-dialog");
});
