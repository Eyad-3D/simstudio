// MOD-43: on the FS example, one click on "FS events" marks its cases for
// the four dynamic events (adding the skidpad), runs them and shows the
// points table; a reference time typed for an event gives its points.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, ribbonTab, showPanel } from "./app";

test("MOD-43: one click runs the four FS events and shows their points", async ({ page }) => {
  test.setTimeout(120_000);
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "FS events" }).click();

  const table = page.getByRole("table", { name: "Formula Student points table" });
  await expect(table).toBeVisible({ timeout: 60_000 });
  // every event ran: each has its time
  for (const ev of ["Acceleration", "Skidpad", "Autocross", "Endurance"]) {
    await expect(table.getByRole("row", { name: new RegExp(`^${ev}`) })).toContainText(/\d+\.\d{3}/, {
      timeout: 60_000,
    });
  }
  // no reference times yet: no points, and the table says what to set
  await expect(table.getByRole("row", { name: /^Skidpad/ })).toContainText("Reference time");

  // the skidpad case: the reference time gives its points on the next run
  await showPanel(page, "Cases & Parameters");
  await page.getByLabel("Case", { exact: true }).selectOption({ label: "Skidpad" });
  await expect(page.locator("label", { hasText: /^FS event/ }).locator("select")).toHaveValue("skidpad");
  await page.locator("label", { hasText: /^Reference time/ }).locator("input").fill("4.9");
  await page.getByRole("button", { name: "Run case" }).click();
  await showPanel(page, "Cases & Parameters");
  await expect(table.getByRole("row", { name: /^Skidpad/ })).toContainText(/\d+\.\d \/ 50/, { timeout: 60_000 });
});
