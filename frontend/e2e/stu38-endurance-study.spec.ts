// STU-38: on the FS example's endurance case, a grid of pack size and power
// cap runs as one study and shows a map, with the runs that run out of
// energy marked DNF, and each pack size runs its own pack.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, showPanel } from "./app";

test("STU-38: a pack size × power cap grid runs as one study with a map", async ({ page }) => {
  test.setTimeout(180_000);
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  await page.getByLabel("Case", { exact: true }).selectOption({ label: "Endurance energy" });
  await page
    .locator("label", { hasText: /^FS event/ })
    .locator("select")
    .selectOption("endurance");
  const study = page.getByRole("region", { name: "Endurance energy study" });
  await study
    .locator("label", { hasText: /^Capacities/ })
    .locator("input")
    .fill("4, 7.2");
  await study
    .locator("label", { hasText: /^Power limits/ })
    .locator("input")
    .fill("30, 60");
  await study.getByRole("button", { name: /Run 2 × 2 = 4 runs/ }).click();
  await showPanel(page, "Cases & Parameters");
  const map = page.getByRole("table", { name: "Endurance energy map" });
  await expect(map).toBeVisible({ timeout: 150_000 });
  await expect(map.getByRole("cell")).toHaveCount(4);
  // a 4 kWh pack does not last 22 km even at 30 kW (the example's pack used
  // 5.3 kWh there), the 7.2 kWh pack does
  const small = map.getByRole("row", { name: /^4/ }).getByRole("cell");
  const large = map.getByRole("row", { name: /^7\.2/ }).getByRole("cell");
  await expect(small.first()).toContainText("DNF");
  await expect(large.first()).not.toContainText("DNF");
  // the pack size changes the run: in the same power column the two packs
  // give different figures (the study once ran one pack size throughout)
  for (const k of [0, 1]) expect(await small.nth(k).textContent()).not.toBe(await large.nth(k).textContent());
});
