// STU-38: on the FS example's endurance case, a grid of pack size and power
// cap runs as one study. Lap cases do not run in this version (NEW-LAP,
// until they are ported), and a study whose Run does nothing is not offered
// (UI 4.43): an endurance case shows no Endurance energy study.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, showPanel } from "./app";

test("STU-38: the pack size × power cap study is not offered in this version", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  await page.getByLabel("Case", { exact: true }).selectOption({ label: "Endurance energy" });
  const event = page.locator("label", { hasText: /^FS event/ }).locator("select");
  await event.selectOption("endurance");
  await expect(event).toHaveValue("endurance");
  await expect(page.getByRole("region", { name: "Endurance energy study" })).toHaveCount(0);
  await expect(page.locator("[data-not-yet]")).toHaveCount(0);
});
