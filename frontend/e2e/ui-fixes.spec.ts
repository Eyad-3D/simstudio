// Defects only a browser shows: the Results legend with the pointer off the
// chart (it read "—") and with the baseline drawn faint (a second legend row
// per channel); a box zoom on a table's plot in a part's parameter window
// (uPlot ended the drag on the main window's page, so it never ended).
import { expect, test, type Page } from "@playwright/test";
import { openApp, openParams, ribbonTab, runActiveCase, selectElement, xRange } from "./app";

const results = (page: Page) => page.locator(".ss-zoom.absolute");

test("the legend reads where each line ended with the pointer away, and the baseline in the channel's own row", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  const legend = results(page).locator(".u-legend");
  await page.mouse.move(2, 2); // away from the chart
  const values = legend.locator(".u-series:visible .u-value");
  await expect(values.first()).toBeVisible();
  for (const v of await values.allTextContents()) expect(v).not.toBe("—");

  // a second run: the first is the baseline, drawn faint, read out in each channel's row
  await ribbonTab(page, "Home").click();
  await selectElement(page, "Vehicle");
  const mass = page.locator("tr", { hasText: "Vehicle Mass" }).locator("input");
  await mass.fill(String(Number(await mass.inputValue()) + 400));
  await mass.press("Tab");
  await runActiveCase(page);
  await page.mouse.move(2, 2);
  const soc = legend.locator(".u-series", { has: page.getByText("SOC", { exact: true }) });
  await expect(soc).toBeVisible();
  await expect(soc.locator(".u-value")).toHaveText(/ · baseline \d/);
  await expect(legend.getByText("baseline · SOC", { exact: true })).toBeHidden();
});

test("a box drawn on a table's plot in a part's window zooms into it", async ({ page }) => {
  await openApp(page);
  const win = await openParams(page, "E-Motor");
  const chart = win.locator('[data-param="drag_torque"]').getByRole("img", { name: /^Table plot: / });
  await expect(chart).toBeVisible();
  const [lo, hi] = await xRange(chart);
  const box = (await chart.locator(".u-over").boundingBox())!;
  const y = box.y + box.height / 2;
  await win.mouse.move(box.x + box.width * 0.3, y);
  await win.mouse.down();
  await win.mouse.move(box.x + box.width * 0.6, y, { steps: 8 });
  await win.mouse.up();
  await expect.poll(async () => (await xRange(chart))[1] - (await xRange(chart))[0]).toBeLessThan((hi - lo) * 0.5);
  // a double-click shows the whole table again
  await chart.locator(".u-over").dblclick();
  await expect.poll(() => xRange(chart)).toEqual([lo, hi]);
});
