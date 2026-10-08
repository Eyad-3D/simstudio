// CON-15: an example opens with its stored reference results already in
// Results, and carries a card that says what it answers and what to expect.
import { expect, test } from "@playwright/test";
import { headlineTile, openApp, openFromMenu, ribbonTab, runActiveCase, runButton } from "./app";

test("CON-15: an example opens with its stored results and its card", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "P2 Hybrid Car");

  // the stored result is there before anything runs
  await ribbonTab(page, "Results").click();
  await expect(headlineTile(page, "Fuel consumption")).toContainText("2.838");
  await expect(page.locator("select").filter({ hasText: "EPA city (UDDS) · Stored result" }).first()).toBeVisible();
  const expected = page.getByRole("region", { name: "Expected values" }).filter({ visible: true }).first();
  await expect(expected).toContainText("Fuel consumption");
  await expect(expected).toContainText("within");

  // Run recomputes it, compared with the stored one
  await runActiveCase(page);
  await expect(runButton(page)).toBeEnabled({ timeout: 60_000 }); // the run has finished
  await expect(headlineTile(page, "Fuel consumption")).toContainText("2.838");
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
