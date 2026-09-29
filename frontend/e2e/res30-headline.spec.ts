// RES-30: a run's headline numbers sit in a strip above the chart, all in view
// at the smallest window LightSim supports, and the plot opens on target
// against actual speed (did the car follow the cycle?), the target dashed and
// drawn on top. The full summary is one keypress away, and opens by itself
// when runs are overlaid.
import { expect, test } from "@playwright/test";
import { openApp, runActiveCase, runButton } from "./app";

test.use({ viewport: { width: 1366, height: 768 } });

test("RES-30: headline numbers in view at 1366×768, plot opens on target vs actual speed", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);

  const headline = page.getByLabel("Headline results");
  for (const name of ["Consumption", "Distance driven", "HV Battery Pack — final SOC"]) {
    const term = headline.getByRole("term").filter({ hasText: name });
    await expect(term).toBeInViewport({ ratio: 1 });
    await expect(term.locator("xpath=following-sibling::dd")).toBeInViewport({ ratio: 1 });
  }
  await expect(headline).toContainText("kWh/100km");

  await expect(page.getByRole("checkbox", { name: /^Target Speed/ })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: /^Vehicle Speed/ })).toBeChecked();
  // the target is drawn last, over the speed that follows it
  await expect(page.getByRole("img", { name: /^Results chart:/ })).toHaveAttribute(
    "aria-label",
    /^Results chart: [^;]*Vehicle Speed[^;]*, Target Speed; /,
  );

  // every summary value stays one keypress away
  const all = page.getByText(/^All summary values/);
  const duration = page.getByRole("cell", { name: "Simulated duration" });
  await expect(duration).toBeHidden();
  await all.focus();
  await page.keyboard.press("Enter");
  await expect(duration).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(duration).toBeHidden();

  // overlaying a run opens it: its columns are the runs side by side
  await runButton(page).click();
  await expect(page.getByText("2 stored runs", { exact: true })).toBeVisible({ timeout: 60_000 });
  await page.getByRole("checkbox", { name: /^City Cycle ·/ }).check();
  await expect(duration).toBeVisible();
});
