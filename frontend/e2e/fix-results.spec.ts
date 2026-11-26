// Defects a pass through the app as a user found in Results, the study
// views, the Signal Plot and the exports (app pass, 26 Nov), each checked
// where a person sees it.
import { expect, test } from "@playwright/test";
import { finishNotice, openApp, runButton } from "./app";

test("app pass #7: the Signal Plot names each channel with its part", async ({ page }) => {
  await openApp(page);
  await runButton(page).click();
  await expect(finishNotice(page)).toBeVisible({ timeout: 60_000 });
  const channel = page.locator("select[title='Channel to plot']");
  if (!(await channel.isVisible())) await page.locator(".dv-tab", { hasText: "Signal Plot" }).first().click();
  const names = await channel.locator("option").allInnerTexts();
  // five parts have a Brake Command: each is named with its part
  expect(names.filter((n) => n.endsWith("· Brake Command [-]"))).toHaveLength(5);
  expect(names).toContain("Brake RL · Brake Command [-]");
  expect(names.filter((n) => !n.includes(" · "))).toEqual([]);
});
