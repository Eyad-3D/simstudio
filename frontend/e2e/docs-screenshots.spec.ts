// The pictures in the README (LRN-03), taken from the real app so they show
// what users get. Not a test: it runs only when asked for, and writes the
// PNGs into docs/screenshots/ with the app version in their names, so a
// picture's age is plain to see.
//
//   npm run build && DOCS_SHOTS=1 npx playwright test docs-screenshots
//
// Then point the README at the new files and delete the old ones.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, selectElement, showPanel } from "./app";
import { openExample } from "./ui-helpers";

test.skip(!process.env.DOCS_SHOTS, "writes the README pictures; set DOCS_SHOTS=1 to take them");
test.use({ viewport: { width: 1600, height: 900 }, locale: "en-US", timezoneId: "UTC" });

const root = new URL("../../", import.meta.url);
const version = readFileSync(new URL("VERSION", root), "utf8").trim();
const shot = (name: string) =>
  fileURLToPath(new URL(`docs/screenshots/lightsim-${version}-${name}.png`, root));

test("README pictures", async ({ page }) => {
  test.setTimeout(240_000);
  await page.clock.setFixedTime(new Date("2026-09-23T09:30:00Z"));

  // 1. the electric-car example on the diagram, with a part's parameters
  await openApp(page);
  await selectElement(page, "E-Motor");
  await page.waitForTimeout(800);
  await page.screenshot({ path: shot("topology") });

  // 2. a finished run: speed against target, battery power and charge
  await runActiveCase(page);
  await expect(page.getByText(/^\d+ stored runs?$/)).toBeVisible();
  await page.waitForTimeout(500);
  await page.screenshot({ path: shot("results") });

  // 3. a parameter sweep of the vehicle's mass, three points
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  const sweepElement = page.locator("select", { has: page.locator("option", { hasText: "Element…" }) }).nth(1);
  await sweepElement.selectOption({ label: "Vehicle" });
  await page.locator("input[type=number][max='16']").fill("3");
  await page.getByRole("button", { name: "Run sweep (3)" }).click();
  await expect(page.getByPlaceholder("Search channels…")).toBeVisible({ timeout: 120_000 });
  await page.getByRole("button", { name: "Sweep", exact: true }).click();
  // energy use against mass says more than the near-flat final charge
  const metric = page.getByTitle("Summary metric to plot against the swept value");
  const labels = await metric.locator("option").allTextContents();
  const consumption = labels.find((l) => /consumption/i.test(l));
  expect(consumption, `a consumption metric among ${labels.join(", ")}`).toBeTruthy();
  await metric.selectOption({ label: consumption! });
  await page.waitForTimeout(800);
  await page.screenshot({ path: shot("sweep") });
});

test("README picture in the dark theme", async ({ page }) => {
  // a fresh window, so no panel opened by the runs above carries over
  await page.addInitScript(() => localStorage.setItem("lightsim-theme", "dark"));
  await openApp(page);
  await openExample(page, "Hybrid");
  await page.waitForTimeout(800);
  await page.screenshot({ path: shot("hybrid-dark") });
});
