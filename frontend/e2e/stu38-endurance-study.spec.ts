// STU-38: on the FS example's endurance case, a grid of pack size and power
// cap runs as one study (a map, the runs that run out of energy marked DNF,
// each pack size its own pack). Lap cases do not run in this version
// (NEW-LAP, until they are ported): the study's Run is shown, dimmed, says
// so, and a press runs nothing.
import { expect, test } from "@playwright/test";
import { finishNotice, openApp, openFromMenu, showPanel } from "./app";

test("STU-38: the pack size × power cap study is set up, but its Run is not pressable in this version", async ({ page }) => {
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
  const run = study.getByRole("button", { name: /Run 2 × 2 = 4 runs/ });
  await expect(run).toHaveAttribute("aria-disabled", "true");
  await expect(run).toHaveAttribute("data-not-yet", "The endurance energy study: not in this version of LightSim yet");
  await run.click({ force: true });
  await expect(finishNotice(page)).toHaveCount(0);
});
