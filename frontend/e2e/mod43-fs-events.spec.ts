// MOD-43: on the FS example, one click on "FS events" marks its cases for
// the four dynamic events (adding the skidpad), runs them and shows the
// points table. Formula Student events and lap cases do not run in this
// version (NEW-FS-EVENT, NEW-LAP, until they are ported): each row says its
// run failed.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, ribbonTab, showPanel } from "./app";

test("MOD-43: one click marks and runs the four FS events, and the table says each run failed", async ({ page }) => {
  test.setTimeout(120_000);
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "FS events" }).click();

  const table = page.getByRole("table", {
    name: "Formula Student points table",
  });
  await expect(table).toBeVisible({ timeout: 60_000 });
  // every event was marked and run: each run was refused
  for (const ev of ["Acceleration", "Skidpad", "Autocross", "Endurance"]) {
    await expect(table.getByRole("row", { name: new RegExp(`^${ev}`) })).toContainText(
      "The run failed: see Messages.",
      { timeout: 60_000 },
    );
  }
  // the skidpad case keeps its event
  await showPanel(page, "Cases & Parameters");
  // (the page stays on the Simulations tab, whose ribbon has a Case list too: UX-21)
  await page.getByRole("region", { name: "Cases" }).getByLabel("Case", { exact: true }).selectOption({ label: "Skidpad" });
  await expect(page.locator("label", { hasText: /^FS event/ }).locator("select")).toHaveValue("skidpad");
});
