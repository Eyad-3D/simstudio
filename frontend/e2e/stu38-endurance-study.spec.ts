// STU-38: on the FS example's endurance case, a grid of pack size and power
// cap runs as one study (a map, the runs that run out of energy marked DNF,
// each pack size its own pack). Lap cases do not run in this version
// (NEW-LAP, until they are ported): the study runs its 4 points and each is
// refused.
import { expect, test } from "@playwright/test";
import { finishNotice, openApp, openFromMenu, showPanel } from "./app";

test("STU-38: a pack size × power cap grid runs as one study, each point refused", async ({ page }) => {
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
  await expect(finishNotice(page)).toContainText("0 of 4 points complete", { timeout: 150_000 });
});
