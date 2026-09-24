// VAL-39: a case of kind Performance is driven at full throttle up to its
// target and reports its time to that speed, instead of ending "Cycle not
// followed" while the car accelerates.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, showPanel } from "./app";
import { importProject } from "./ui-helpers";

test("VAL-39: a 0-100 km/h step set to Performance reports its time and says so in Run info", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  const c = example.cases[0];
  c.duration = 20;
  c.parameterOverrides = { ...c.parameterOverrides, "el-task": { profile: "0:100; 20:100" } };
  await importProject(page, { ...example, id: `e2e-val39-${n}`, name: `E2E performance ${n}` });

  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  const kind = page.locator("label", { hasText: /^Kind/ }).locator("select");
  await expect(kind).toHaveValue("cycle");
  await kind.selectOption("performance");

  await runActiveCase(page);
  await expect(page.getByText("Time to 100 km/h").first()).toBeVisible();
  await expect(page.getByText(/Cycle not followed/)).toHaveCount(0);
  await page.getByTitle(/^Run info/).click();
  await expect(page.getByRole("region", { name: "Run info" })).toContainText("performance test");
});
