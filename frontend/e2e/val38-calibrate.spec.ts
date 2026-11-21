// VAL-38: calibrate lap mode on one logged lap and check it on another.
// Calibrating runs lap cases, which this version does not run (NEW-LAP,
// until they are ported): the button is shown, dimmed, says so on hover and
// focus, and a press opens nothing.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, ribbonTab } from "./app";

test("VAL-38: Calibrate lap is shown but not pressable in this version, and says why", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await ribbonTab(page, "Simulations").click();
  const button = page.getByRole("button", { name: "Calibrate lap" });
  await expect(button).toHaveAttribute("aria-disabled", "true");
  await expect(button).toHaveAttribute("data-not-yet", "Calibrate lap: not in this version of LightSim yet");
  await button.click({ force: true });
  await expect(page.getByRole("dialog", { name: "Calibrate lap mode" })).toHaveCount(0);
});
