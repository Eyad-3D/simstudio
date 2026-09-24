// RES-03: the Signal Plot follows a paced run while it runs. Its data used to
// be memoised on the channel object, which the live run grows in place, so the
// plot stayed on the first flush (one point at t = 0) until the run ended.
import { expect, test, type Page } from "@playwright/test";
import { runButton, showPanel } from "./app";
import { openApp } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

/** The largest t on the Signal Plot's x-axis. */
async function plotEnd(page: Page): Promise<number> {
  const plot = page.locator(".dv-groupview", { has: page.locator("select[title='Channel to plot']") });
  const ticks = await plot.locator(".recharts-xAxis-tick-labels .recharts-cartesian-axis-tick-value").allTextContents();
  return Math.max(-1, ...ticks.map(Number).filter(Number.isFinite));
}

test("RES-03: the Signal Plot's time axis grows during a 1x paced run", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Cases & Parameters");
  await page.locator("label", { hasText: "Pacing" }).locator("select").selectOption("1");
  await runButton(page).click();
  await showPanel(page, "Signal Plot");

  // at 1x the solver is a few seconds in after a few seconds
  await expect.poll(() => plotEnd(page), { timeout: 20_000 }).toBeGreaterThanOrEqual(3);

  await page.getByTitle("Stop the running simulation", { exact: true }).click();
  await expect(page.getByTitle("Stop the running simulation", { exact: true })).toHaveCount(0, { timeout: 30_000 });
});
