// CON-13: where each parameter value comes from, shown and recorded in Properties.
import { expect, test } from "@playwright/test";
import { openApp, showPanel } from "./app";

test("CON-13: the example's values show their source, and a new one can be recorded", async ({ page }) => {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "Vehicle" }).first().click();
  await showPanel(page, "Properties");
  const sources = page.getByTestId("value-sources");
  // mass, Cd and frontal area come from FASTSim's Cupra Born file
  await expect(sources.locator("summary")).toHaveText(/^Value sources: 3 of \d+ recorded, \d+ at the library default$/);
  await expect(page.getByLabel(/^Source: datasheet · confidence 1/).first()).toBeVisible();
  await sources.locator("summary").click();
  await sources.getByLabel("Parameter to record a source for").selectOption({ label: "Wheelbase" });
  await sources.getByLabel("Source", { exact: true }).fill("Workshop drawing, 2026-10-07");
  await sources.getByLabel("Kind of source").selectOption("measured");
  await sources.getByLabel("Confidence").selectOption("2");
  await sources.getByRole("button", { name: "Save source" }).click();
  await expect(sources.locator("summary")).toHaveText(/^Value sources: 4 of \d+ recorded/);
  await expect(sources.getByText("measured, confidence 2")).toBeVisible();
  await sources.getByRole("button", { name: "Forget the source of Wheelbase" }).click();
  await expect(sources.locator("summary")).toHaveText(/^Value sources: 3 of /);
});
