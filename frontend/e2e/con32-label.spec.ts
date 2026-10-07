// CON-32: a US window-sticker estimate from the EPA city and highway runs,
// with every step shown and marked as not certified.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab } from "./app";

test("CON-32: the electric example gets a label estimate, step by step, not certified", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "US label" }).click();
  const dialog = page.getByRole("dialog", { name: "US label estimate" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Run UDDS and HWFET" }).click();
  await expect(dialog.getByTestId("label-not-certified")).toHaveText("Simulated estimate, not a certified value.", {
    timeout: 60_000,
  });
  await expect(dialog.getByText("Runs: US label: EPA city (UDDS) and US label: EPA highway (HWFET).")).toBeVisible();
  for (const step of ["Lab city energy (battery)", "Label combined MPGe", "Label range"]) {
    await expect(dialog.getByRole("cell", { name: step, exact: true }).first()).toBeVisible();
  }
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});
