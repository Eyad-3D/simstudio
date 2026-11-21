// MOD-43: "FS events" marks the four dynamic events' cases and runs them.
// Three of the four are scored from a lap case, which does not run in this
// version (NEW-FS-EVENT, NEW-LAP, until lap cases are ported), so the button
// is shown, dimmed, says so on hover and focus, and a press does nothing.
// The acceleration is scored without a lap: an Acceleration case's own FS
// event can be set, and its run scores it.
import { expect, test } from "@playwright/test";
import { finishNotice, openApp, openFromMenu, ribbonTab, showPanel } from "./app";

test("MOD-43: FS events is shown but not pressable in this version, and says why", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  const options = page.getByLabel("Case", { exact: true }).first().locator("option");
  await expect(options).not.toHaveCount(0);
  const cases = await options.allTextContents();
  await ribbonTab(page, "Simulations").click();
  const button = page.getByRole("button", { name: "FS events" });
  await expect(button).toHaveAttribute("aria-disabled", "true");
  await expect(button).toHaveAttribute("data-not-yet", "FS events: not in this version of LightSim yet");
  await button.focus();
  await page.keyboard.press("Enter");
  await button.click({ force: true });
  await expect(finishNotice(page)).toHaveCount(0);
  // no case was marked or added
  expect(await options.allTextContents()).toEqual(cases);
});

test("MOD-43: an Acceleration case's FS event can be set", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  await page.getByLabel("Case", { exact: true }).first().selectOption("case-accel-75m");
  const event = page.getByLabel("FS event");
  await expect(event).toBeEnabled();
  await expect(event).not.toHaveAttribute("aria-disabled", "true");
  await event.selectOption("acceleration");
  await expect(event).toHaveValue("acceleration");
});
