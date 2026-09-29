// STU-37: one click on "Acceleration test" adds a 75 m acceleration case
// (staged 0.30 m behind the start line, 25 s time limit), runs it and shows
// the time, the speed at the line and that the results are estimates.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab } from "./app";

test("STU-37: one click gives the 75 m time and the speed at the line within 5 s", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Simulations").click();
  const button = page.getByRole("button", { name: "Acceleration test" });
  await expect(button).toBeEnabled();

  const t0 = Date.now();
  await button.click();
  await expect(page.getByText("Time to 75 m", { exact: true })).toBeVisible({ timeout: 5000 });
  await expect(page.getByText("Speed at 75 m", { exact: true })).toBeVisible({ timeout: 5000 });
  expect(Date.now() - t0).toBeLessThan(5000);

  await expect(page.getByRole("row", { name: /^Time to 75 m/ })).toContainText("pass");
  await expect(page.getByText("Summary value · estimate")).toBeVisible();
  await expect(page.getByText(/Cycle not followed/)).toHaveCount(0);
  await page.getByTitle(/^Run info/).click();
  await expect(page.getByRole("region", { name: "Run info" })).toContainText(
    "acceleration test over 75 m, start line 0.3 m (estimate)",
  );
});
