// CON-15: an example opens with its stored reference results already in
// Results, and carries a card that says what it answers and what to expect.
import { expect, test } from "@playwright/test";
import { headlineTile, openApp, openFromMenu, ribbonTab, runActiveCase, runButton, runRow } from "./app";

test("CON-15: an example opens with its stored results and its card", async ({ page }) => {
  test.setTimeout(240_000);
  await openApp(page);
  await openFromMenu(page, "P2 Hybrid Car");

  // the stored result is there before anything runs
  await ribbonTab(page, "Results").click();
  await expect(headlineTile(page, "Fuel consumption")).toContainText("2.809");
  // its row in the table of runs names it, on its case
  const stored = runRow(page, 0).row;
  await expect(stored).toContainText("Stored result");
  await expect(stored).toContainText("EPA city (UDDS)");
  const expected = page.getByRole("region", { name: "Expected values" }).filter({ visible: true }).first();
  await expect(expected).toContainText("Fuel consumption");
  await expect(expected).toContainText("within");

  // Run recomputes it, charge-balanced, compared with the stored one (the
  // stored result is the app's engine's own run, so Run gives its figure)
  await runActiveCase(page);
  await expect(runButton(page)).toBeEnabled({ timeout: 150_000 }); // the run has finished
  await expect(headlineTile(page, "Fuel consumption")).toContainText("2.809");
  await expect(page.getByText(/vs baseline/).first()).toBeVisible();

  // the card, in the Project tab
  await ribbonTab(page, "Project").click();
  await page.getByRole("button", { name: /Card/ }).click();
  const card = page.getByRole("dialog", { name: "Project card" });
  await expect(card.locator("textarea").first()).toHaveValue(/parallel hybrid/);
  await expect(card).toContainText("EPA 2022 Test Car List");
  await card.getByRole("button", { name: "Cancel" }).click();
  await expect(card).toHaveCount(0);
});
