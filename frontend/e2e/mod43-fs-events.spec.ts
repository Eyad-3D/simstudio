// MOD-43: "FS events" marks the four dynamic events' cases and runs them.
// Formula Student events and lap cases do not run in this version
// (NEW-FS-EVENT, NEW-LAP, until they are ported): a button that does nothing
// is not offered (UI 4.43), so FS events is not in the ribbon, and the
// example's cases stay as they are.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, ribbonTab, showPanel } from "./app";

test("MOD-43: FS events is not offered in this version", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  const options = page.getByLabel("Case", { exact: true }).first().locator("option");
  await expect(options).not.toHaveCount(0);
  const cases = await options.allTextContents();
  await ribbonTab(page, "Simulations").click();
  await expect(page.getByRole("button", { name: "Acceleration test" })).toBeVisible();
  await expect(page.getByRole("button", { name: "FS events" })).toHaveCount(0);
  // no case was marked or added
  expect(await options.allTextContents()).toEqual(cases);
});
