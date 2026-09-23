// UX-09, the part the 0.2 audit found stale: the status bar's error count and
// the red part badges show the problems the model has now. Once the model has
// been checked, every edit re-checks it by itself, so a fixed problem clears
// without pressing Data Checks again.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, showPanel } from "./app";

test("UX-09: a fixed problem clears its badge and the status-bar count by itself", async ({ page }) => {
  await openApp(page);
  const motor = page.locator(".react-flow__node", { hasText: "E-Motor" }).first();
  const badge = motor.getByTitle(/has no Traction Command signal/);
  const count = page.getByText(/^\d+ error(s|\(s\))?$/);
  const runChecks = page.getByRole("button", { name: "Run Data Checks" });

  // unwire the E-Motor's command and check the model
  await showPanel(page, "Data Bus Connections");
  await page
    .locator("div:has(> button[title='Remove connection'])", { hasText: "E-Motor · Traction Command" })
    .getByTitle("Remove connection")
    .click();
  await showPanel(page, "Data Checks");
  await runChecks.click();
  await expect(badge).toBeVisible();
  await expect(count).toHaveText("1 error");

  // the count leads to the list of problems
  await showPanel(page, "Messages");
  await expect(runChecks).toBeHidden();
  await count.click();
  await expect(runChecks).toBeVisible();

  // put the link back: the badge and the count clear without Data Checks
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(badge).toHaveCount(0, { timeout: 2000 });
  await expect(count).toHaveCount(0, { timeout: 2000 });
});
