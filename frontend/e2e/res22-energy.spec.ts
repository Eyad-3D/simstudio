// RES-22, RES-38, RES-39, UX-41: a run's Energy view (a Sankey chart and a
// table per part), its Duty view, the band that says what held the car
// back, the diagram's Energy labels and bar chart, and the marks on what
// changed since the run shown in Results.
import { expect, test } from "@playwright/test";
import { expectProject, openApp, ribbonTab, runActiveCase, selectElement, showPanel } from "./app";
import { importProject } from "./ui-helpers";

test.use({ viewport: { width: 1366, height: 768 } });

test("RES-22/38/39 and UX-41: energy, duty and limits of a run, and what changed since", async ({ page }) => {
  const n = Date.now() % 100000;
  const name = `E2E energy ${n}`;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  // (the City Cycle brakes from 300 s on)
  await importProject(page, { ...example, id: `e2e-res22-${n}`, name });
  await expectProject(page, name, { unsaved: true });
  await runActiveCase(page);

  // what limits the car: a legend under the chart, its times adding up to the run
  const limits = page.getByRole("region", { name: "What limits the car" });
  await expect(limits).toContainText("Braking");
  await expect(limits).toContainText("Driver's demand met");

  // Energy: the Sankey from the battery, and every part's books
  await page.getByRole("button", { name: "Energy", exact: true }).click();
  const sankey = page.getByRole("img", { name: /^Energy Sankey chart: HV Battery Pack/ });
  await expect(sankey).toBeVisible();
  await expect(sankey).toContainText("Air drag");
  await expect(sankey).toContainText("Rolling resistance");
  await expect(page.getByText(/^Not accounted for/).first()).toBeVisible();
  const table = page.getByRole("table", { name: "Energy per part" });
  await expect(table).toContainText("HV Battery Pack");
  await expect(table).toContainText("E-Motor");
  const csv = page.waitForEvent("download");
  await page.getByTitle("Save the energy table and the chart's bands as CSV").click();
  expect((await csv).suggestedFilename()).toMatch(/energy\.csv$/);

  // Duty: RMS beside the peaks
  await page.getByRole("button", { name: "Duty", exact: true }).click();
  const duty = page.getByRole("table", { name: "Duty per part" });
  await expect(duty).toContainText("Shaft power");
  await expect(duty.getByRole("columnheader", { name: "RMS" })).toBeVisible();

  // the diagram's Energy labels and the bar chart of losses
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Topology");
  await page.getByTitle(/^Energy: label each part/).click();
  const bars = page.getByRole("region", { name: "Energy lost per part" });
  await expect(bars).toContainText("E-Motor");
  await expect(page.locator("[data-energy-label]").first()).toContainText(/lost [\d.,]+ kWh\s*in [\d.,]+ · out [\d.,]+/);
  await bars.getByRole("button", { name: /^E-Motor/ }).click();
  await expect(page.locator(".react-flow__node.selected")).toHaveText(/E-Motor/);
  await page.getByTitle("Hide the energy labels and this chart").click();
  await expect(bars).toBeHidden();

  // UX-41: an edit marks the part and Results counts it
  const dots = page.getByRole("img", { name: "Changed since the results shown" });
  await expect(dots).toHaveCount(0);
  await selectElement(page, "Vehicle");
  const mass = page.locator("tr", { hasText: "Vehicle Mass" }).locator("input");
  await mass.fill("2300");
  await mass.press("Tab");
  await expect(page.locator(".react-flow__node", { hasText: "Vehicle" }).getByRole("img", { name: "Changed since the results shown" })).toBeVisible();
  await ribbonTab(page, "Results").click();
  const banner = page.getByRole("status", { name: "Results out of date" });
  await expect(banner).toContainText("These results are from before 1 change to the model.");
  await banner.getByRole("button", { name: "Show changes" }).click();
  await expect(banner).toContainText("Vehicle · Vehicle Mass 1,927 → 2,300 kg");

  // a new run clears the marks
  await banner.getByRole("button", { name: "Re-run" }).click();
  await expect(page.getByText("2 stored runs", { exact: true })).toBeVisible({ timeout: 60_000 });
  await expect(banner).toBeHidden();
  await expect(dots).toHaveCount(0);
});
