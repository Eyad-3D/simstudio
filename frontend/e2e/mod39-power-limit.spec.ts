// MOD-39: the battery's Formula Student preset sets its Output Power Limit
// and Voltage Class in one step, and the run summary checks them with a
// pass/fail marker.
import { expect, test } from "@playwright/test";
import { headlineTile, openApp, openSummary, runActiveCase, selectElement } from "./app";
import { importProject } from "./ui-helpers";

test("MOD-39: the Formula Student preset holds the battery to 80 kW and the summary says pass", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  const c = example.cases[0];
  c.kind = "performance";
  c.duration = 20;
  c.parameterOverrides = { ...c.parameterOverrides, "el-task": { profile: "0:100; 20:100" } };
  await importProject(page, { ...example, id: `e2e-mod39-${n}`, name: `E2E power limit ${n}` });

  await selectElement(page, "HV Battery Pack");
  await expect(page.getByText(/^FS Rules 2026 v1\.1 \(FSG\): 80 kW/)).toBeVisible();
  await page.getByRole("button", { name: "Apply preset: Formula Student Electric" }).click();
  await expect(page.getByRole("spinbutton", { name: "Output Power Limit (0 = none)" })).toHaveValue("80");
  await expect(page.getByRole("spinbutton", { name: "Voltage Class (0 = none)" })).toHaveValue("600");

  await runActiveCase(page);
  await expect(headlineTile(page, "Time to 100 km/h")).toBeVisible();
  await openSummary(page);
  const row = page.getByRole("row", { name: /HV Battery Pack — peak terminal power, averaged/ });
  await expect(row).toContainText("pass");
  await expect(row).toContainText("≤ 80");
});
