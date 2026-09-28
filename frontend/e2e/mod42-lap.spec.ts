// MOD-42: a case of kind Lap runs the model's Race Track. Its layout and laps
// are the case's own (overrides of the Race Track), Duration, Step and Pacing
// do not apply, and the summary leads with the lap time, marked an estimate.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, showPanel } from "./app";
import { importProject } from "./ui-helpers";

async function importBev(page: import("@playwright/test").Page, withTrack: boolean) {
  const n = Date.now() % 100000;
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  if (withTrack) {
    example.systems[0].elements.push({
      id: "el-track", componentDefId: "track.lap", label: "Race Track",
      position: { x: 900, y: 40 }, parameterOverrides: {},
    });
  }
  await importProject(page, { ...example, id: `e2e-mod42-${n}`, name: `E2E lap ${n}` });
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
}

test("MOD-42: a lap case on the Skidpad gives its lap and sector times as estimates", async ({ page }) => {
  await openApp(page);
  await importBev(page, true);
  const kind = page.locator("label", { hasText: /^Kind/ }).locator("select");
  await kind.selectOption("lap");
  await expect(page.locator("label", { hasText: /^Duration/ }).locator("input")).toBeDisabled();
  await expect(page.locator("label", { hasText: /^Pacing/ }).locator("select")).toBeDisabled();

  const layout = page.locator("label", { hasText: /^Track layout/ }).locator("select");
  await expect(layout).toHaveValue("Autocross");
  await layout.selectOption("Skidpad");
  await page.locator("label", { hasText: /^Laps/ }).locator("input").fill("2");
  await ribbonTab(page, "Simulations").click();
  await expect(page.getByTitle("Solver settings for this case")).toHaveText("lap mode");

  await runActiveCase(page);
  await expect(page.getByRole("row", { name: /^Lap time/ })).toBeVisible();
  await expect(page.getByRole("row", { name: /^Sector 2 time/ })).toBeVisible();
  await expect(page.getByText("Summary value · estimate")).toBeVisible();
  await page.getByTitle(/^Run info/).click();
  const info = page.getByRole("region", { name: "Run info" });
  await expect(info).toContainText("lap mode (estimate)");
  await expect(info).toContainText("Race Track · Layout = Skidpad");
});

test("MOD-42: a lap case without a Race Track says where to get one", async ({ page }) => {
  await openApp(page);
  await importBev(page, false);
  await page.locator("label", { hasText: /^Kind/ }).locator("select").selectOption("lap");
  await expect(page.getByText("Add a Race Track from Driver & Signals")).toBeVisible();
});
