// MOD-42: a case of kind Lap runs the model's Race Track. Its layout and laps
// are the case's own (overrides of the Race Track), and Duration, Step and
// Pacing do not apply. Lap cases do not run in this version (the app's engine
// refuses them, NEW-LAP, until they are ported): the run says so.
import { expect, test } from "@playwright/test";
import { expectRefused, openApp, ribbonTab, runButton, showPanel } from "./app";
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

test("MOD-42: a lap case on the Skidpad takes its layout and laps, and its run is refused", async ({ page }) => {
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

  await runButton(page).click();
  await expectRefused(page, "NEW-LAP");
});

test("MOD-42: a lap case without a Race Track says where to get one", async ({ page }) => {
  await openApp(page);
  await importBev(page, false);
  await page.locator("label", { hasText: /^Kind/ }).locator("select").selectOption("lap");
  await expect(page.getByText("Add a Race Track from Driver & Signals")).toBeVisible();
});
