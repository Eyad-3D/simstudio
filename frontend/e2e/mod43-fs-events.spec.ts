// MOD-43: "FS events" marks the four dynamic events' cases and runs them.
// Formula Student events and lap cases do not run in this version
// (NEW-FS-EVENT, NEW-LAP, until they are ported), so the button is shown,
// dimmed, says so on hover and focus, and a press does nothing.
import { expect, test } from "@playwright/test";
import { finishNotice, openApp, openFromMenu, ribbonTab, showPanel } from "./app";

test("MOD-43: FS events is shown but not pressable in this version, and says why", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  const cases = await page.getByLabel("Case", { exact: true }).first().locator("option").allTextContents();
  await ribbonTab(page, "Simulations").click();
  const button = page.getByRole("button", { name: "FS events" });
  await expect(button).toHaveAttribute("aria-disabled", "true");
  await expect(button).toHaveAttribute("data-not-yet", "FS events: not in this version of LightSim yet");
  await button.focus();
  await page.keyboard.press("Enter");
  await button.click({ force: true });
  await expect(finishNotice(page)).toHaveCount(0);
  // no case was marked or added
  expect(await page.getByLabel("Case", { exact: true }).first().locator("option").allTextContents()).toEqual(cases);
});
