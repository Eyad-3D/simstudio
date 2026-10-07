// STU-38: on the FS example's endurance case, a grid of pack size and power
// cap runs as one study and shows a map, with the runs that run out of
// energy marked DNF.
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
  // a 4 kWh pack does not last 22 km at 60 kW
  await expect(map.getByRole("row", { name: /^4/ })).toContainText("DNF");
  await expect(map.getByRole("row", { name: /^7\.2/ }).getByRole("cell").first()).not.toContainText("DNF");
});
