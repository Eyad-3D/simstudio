// CON-15: an example opens with its stored reference results already in
// Results, and carries a card that says what it answers and what to expect.
import { expect, test } from "@playwright/test";
import { expectRefused, headlineTile, openApp, openFromMenu, ribbonTab, runButton } from "./app";

test("CON-15: an example opens with its stored results and its card", async ({ page }) => {
  test.setTimeout(240_000);
  await openApp(page);
  await openFromMenu(page, "P2 Hybrid Car");

  // the stored result is there before anything runs
  await ribbonTab(page, "Results").click();
  await expect(headlineTile(page, "Fuel consumption")).toContainText("2.827");
  await expect(page.locator("select").filter({ hasText: "EPA city (UDDS) · Stored result" }).first()).toBeVisible();
  const expected = page.getByRole("region", { name: "Expected values" }).filter({ visible: true }).first();
  await expect(expected).toContainText("Fuel consumption");
  await expect(expected).toContainText("within");

  // Run would recompute it, but the hybrid's charge-balanced case does not
  // run in this version (NEW-BALANCE, until it is ported): the run says so
  await runButton(page).click();
  await expectRefused(page, "NEW-BALANCE");

  // the card, in the Project tab
  await ribbonTab(page, "Project").click();
  await page.getByRole("button", { name: /Card/ }).click();
  const card = page.getByRole("dialog", { name: "Project card" });
  await expect(card.locator("textarea").first()).toHaveValue(/parallel hybrid/);
  await expect(card).toContainText("EPA 2022 Test Car List");
  await card.getByRole("button", { name: "Cancel" }).click();
  await expect(card).toHaveCount(0);
});
