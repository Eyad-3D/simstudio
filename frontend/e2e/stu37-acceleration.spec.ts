// STU-37: one click on "Acceleration test" adds a 75 m acceleration case
// (staged 0.30 m behind the start line, 25 s time limit), runs it and shows
// the time, the speed at the line and that the results are estimates.
import { expect, test } from "@playwright/test";
import { headlineTile, openApp, openSummary, ribbonTab, showPanel } from "./app";

test("STU-37: one click gives the 75 m time and the speed at the line within 5 s", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Simulations").click();
  const button = page.getByRole("button", { name: "Acceleration test" });
  await expect(button).toBeEnabled();

  const t0 = Date.now();
  await button.click();
  // the headline numbers above the chart
  await expect(headlineTile(page, "Time to 75 m")).toBeVisible({ timeout: 5000 });
  await expect(headlineTile(page, "Speed at 75 m")).toBeVisible({ timeout: 5000 });
  expect(Date.now() - t0).toBeLessThan(5000);

  await expect(headlineTile(page, "Time to 75 m")).toContainText("pass");
  await expect(page.getByTitle(/results are estimates/).filter({ hasText: /^estimates$/ })).toBeVisible();
  await openSummary(page);
  await expect(page.getByRole("row", { name: /^Time to 75 m/ })).toContainText("pass");
  await expect(page.getByText("Summary value · estimate")).toBeVisible();
  await expect(page.getByText(/Cycle not followed/)).toHaveCount(0);
  await page.getByTitle(/^Run info/).click();
  await expect(page.getByRole("region", { name: "Run info" })).toContainText(
    "acceleration test over 75 m, start line 0.3 m (estimate)",
  );
});

test("STU-37: an acceleration case's own numbers turn red outside their limits as they are typed", async ({
  page,
}) => {
  await openApp(page);
  await showPanel(page, "Cases & Parameters");
  await page.locator("label", { hasText: /^Kind/ }).locator("select").selectOption("acceleration");
  const distance = page.locator("label", { hasText: /^Distance/ }).locator("input");
  await expect(distance).toHaveValue("75");
  await distance.fill("0");
  await expect(distance).toHaveAttribute("aria-invalid", "true", { timeout: 500 });
  await expect(page.getByRole("alert").filter({ hasText: "Distance must be" })).toHaveText(
    "Distance must be above 0 m.",
  );
  await distance.fill("75");
  await expect(distance).not.toHaveAttribute("aria-invalid");

  // an empty reference time is none; 0 or less is not a time
  const reference = page.locator("label", { hasText: /^Reference time/ }).locator("input");
  await reference.fill("-1");
  await expect(page.getByRole("alert").filter({ hasText: "Reference time must be" })).toHaveText(
    "Reference time must be above 0 s.",
  );
  await reference.fill("");
  await reference.blur();
  await expect(reference).toHaveValue("");
  await expect(reference).not.toHaveAttribute("aria-invalid");
});
