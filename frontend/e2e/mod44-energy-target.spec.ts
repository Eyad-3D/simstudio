// MOD-44: an energy target on the FS example's endurance case: lap mode
// lifts and coasts before the braking points to end within 2 % of it.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, openSummary, showPanel } from "./app";

test("MOD-44: an endurance ends within 2 % of its energy target", async ({ page }) => {
  test.setTimeout(120_000);
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  await page.getByLabel("Case", { exact: true }).selectOption({ label: "Endurance energy" });
  await page.locator("label", { hasText: /^Energy target/ }).locator("input").fill("5");
  await page.getByRole("button", { name: "Run case" }).click();
  await openSummary(page);
  const row = page.getByRole("row", { name: /^Energy used against the target/ });
  await expect(row).toBeVisible({ timeout: 60_000 });
  const pct = Number((await row.innerText()).match(/-?[\d.]+(?=\s*%)/)?.[0]);
  expect(Math.abs(pct)).toBeLessThan(2);
  await expect(page.getByRole("row", { name: /^Lift-and-coast, mean share/ })).toBeVisible();
});
