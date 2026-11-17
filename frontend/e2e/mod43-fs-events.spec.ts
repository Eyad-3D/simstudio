// MOD-43: on the FS example, one click on "FS events" marks its cases for
// the four dynamic events (adding the skidpad), runs them and shows the
// points table. Formula Student events and lap cases do not run in this
// version (NEW-FS-EVENT, NEW-LAP, until they are ported): the first event's
// run is refused, which stops the others, and the table says so.
import { expect, test } from "@playwright/test";
import { finishNotice, openApp, openFromMenu, ribbonTab, showPanel } from "./app";

test("MOD-43: one click marks the four FS events and runs them; the table says the first was refused", async ({ page }) => {
  test.setTimeout(120_000);
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "FS events" }).click();
  await expect(finishNotice(page)).toContainText("Formula Student events: Acceleration failed, see Messages.", {
    timeout: 60_000,
  });
  await showPanel(page, "Cases & Parameters");
  const table = page.getByRole("table", {
    name: "Formula Student points table",
  });
  await expect(table).toBeVisible();
  // the four cases are marked; the first event's run was refused, which stops the others
  // (the Acceleration row reads the example's stored run, a run of today's engine)
  for (const ev of ["Skidpad", "Autocross", "Endurance"]) {
    await expect(table.getByRole("row", { name: new RegExp(`^${ev}`) })).toContainText("Not run yet.");
  }
  await showPanel(page, "Problems");
  await expect(page.getByText("Not run [NEW-FS-EVENT]: ").first()).toBeVisible();
  await showPanel(page, "Cases & Parameters");
  // the skidpad case keeps its event
  await showPanel(page, "Cases & Parameters");
  // (the page stays on the Simulations tab, whose ribbon has a Case list too: UX-21)
  await page.getByRole("region", { name: "Cases" }).getByLabel("Case", { exact: true }).selectOption({ label: "Skidpad" });
  await expect(page.locator("label", { hasText: /^FS event/ }).locator("select")).toHaveValue("skidpad");
});
