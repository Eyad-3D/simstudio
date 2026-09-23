// RES-03: the Signal Plot and the Results chart draw on the first run of a
// session, including a first run started from the empty Results page.
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonButton } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

// the Results page is the absolutely positioned overlay beside the Home dock
const results = (page: Page) => page.locator(".ss-zoom.absolute");

test("RES-03: after the first run both the Results chart and the Signal Plot draw", async ({ page }) => {
  await openApp(page);
  await page.locator(".react-flow__pane").first().click({ position: { x: 10, y: 10 } });
  await page.keyboard.press("Control+Enter");
  await expect(results(page).locator(".recharts-line-curve").first()).toBeAttached({ timeout: 60_000 });

  await ribbonButton(page, "Home");
  // the first run opens the tray on the Signal Plot; show it if it is not in front
  const channel = page.locator("select[title='Channel to plot']");
  if (!(await channel.isVisible())) await page.locator(".dv-tab", { hasText: "Signal Plot" }).first().click();
  await expect(channel).toBeVisible();
  expect(await channel.inputValue()).not.toBe("");
  const plot = page.locator(".dv-groupview", { has: channel });
  await expect(plot.locator(".recharts-line-curve").first()).toBeAttached();
});

test("RES-03: a first run started from the empty Results page is drawn", async ({ page }) => {
  await openApp(page);
  await ribbonButton(page, "Results");
  await page.getByRole("button", { name: /Run active case/ }).first().click();
  // the default channel pick waits for the run's channels (it used to store
  // an empty pick from the run's first, channel-less result)
  await expect(results(page).locator(".recharts-line-curve").first()).toBeAttached({ timeout: 60_000 });
});
