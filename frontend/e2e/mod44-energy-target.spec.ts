// MOD-44: an energy target on the FS example's endurance case (lap mode
// lifts and coasts before the braking points to end within 2 % of it). Lap
// cases do not run in this version (NEW-LAP, until they are ported): the run
// says so.
import { expect, test } from "@playwright/test";
import { expectRefused, openApp, openFromMenu, showPanel } from "./app";

test("MOD-44: an endurance takes its energy target, and its run is refused", async ({ page }) => {
  test.setTimeout(120_000);
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  await page.getByLabel("Case", { exact: true }).selectOption({ label: "Endurance energy" });
  await page
    .locator("label", { hasText: /^Energy target/ })
    .locator("input")
    .fill("5");
  await expect(page.locator("label", { hasText: /^Energy target/ }).locator("input")).toHaveValue("5");
  await page.getByRole("button", { name: "Run case" }).click();
  await expectRefused(page, "NEW-LAP");
});
