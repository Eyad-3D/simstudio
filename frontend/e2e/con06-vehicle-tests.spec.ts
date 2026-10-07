// CON-06: one-click vehicle tests on the model as it is.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab } from "./app";

test("CON-06: chosen vehicle tests give their figures and what limits the top speed", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "Vehicle tests" }).click();
  const dialog = page.getByRole("dialog", { name: "Vehicle tests" });
  for (const name of ["Consumption and range at 50, 90 and 120 km/h", "Steepest grade at 30 km/h", "Virtual coast-down: road load A, B, C", "80-120 km/h"]) {
    await dialog.getByRole("checkbox", { name }).uncheck();
  }
  await dialog.getByRole("button", { name: "Run the tests" }).click();
  await expect(dialog.getByRole("cell", { name: "0-100 km/h", exact: true })).toBeVisible({ timeout: 60_000 });
  await expect(dialog.getByText("limited by E-Motor's maximum speed (16,000 1/min)")).toBeVisible();
  await expect(dialog.getByRole("cell", { name: /^7\.1\d* s$/ })).toBeVisible();
});
