// VAL-35: type in a number you trust for a result of a case; every run of
// the case then shows how far it lands, with the hand calculations beside it.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, showPanel } from "./app";
import { importProject } from "./ui-helpers";

test("VAL-35: an expected value is added in seconds and graded after every run", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 60;
  await importProject(page, { ...example, id: `e2e-val35-${n}`, name: `E2E expected ${n}` });

  // a first run names the results that can be checked
  await runActiveCase(page);
  const strip = page.getByRole("region", { name: "Expected values" }).first();
  await expect(strip).toContainText("Hand calculations: 2 passed");

  const started = Date.now();
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  // the example's own expected value comes first; the new row is the last
  const kpi = page.getByLabel("Result (summary value)").last();
  await kpi.fill("Distance driven");
  await page.locator("label", { hasText: /^Expected/ }).locator("input").last().fill("0.2");
  await page.getByLabel("Tolerance", { exact: true }).last().fill("0.05");
  await page.getByLabel("Tolerance in").last().selectOption("unit");
  await page.getByPlaceholder(/^Source, e.g./).last().fill("hand calculation");
  expect(Date.now() - started).toBeLessThan(30_000);

  await runActiveCase(page);
  const mine = page.getByRole("region", { name: "Expected values" }).first().locator("li", { hasText: "Distance driven" });
  await expect(mine).toContainText("Distance driven");
  await expect(mine).toContainText(/outside|near|within/);
  await expect(mine).toContainText("km");

  // and Run info lists it with the run
  await page.getByTitle(/^Run info/).click();
  await expect(page.getByRole("region", { name: "Run info" })).toContainText("hand calculation");
});
