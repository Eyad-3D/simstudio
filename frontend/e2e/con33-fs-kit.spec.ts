// CON-33: the Formula Student example opens from the Open menu on its 75 m
// acceleration case, and one Run gives the 75 m time, held at the 80 kW
// power limit, within 5 s.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, runButton } from "./app";

test("CON-33: the FS example runs its 75 m test from the Open menu within 5 s", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await expect(page.locator(".react-flow__node", { hasText: "Accumulator" })).toBeVisible();
  await expect(page.getByTitle("Active simulation case")).toHaveValue("case-accel-75m");

  const t0 = Date.now();
  await runButton(page).click();
  await expect(page.getByRole("row", { name: /^Time to 75 m/ })).toContainText("pass", { timeout: 5000 });
  expect(Date.now() - t0).toBeLessThan(5000);
  await expect(page.getByRole("row", { name: /^Accumulator — peak terminal power, averaged/ })).toContainText(
    "pass",
  );
  await expect(page.getByText(/Cycle not followed/)).toHaveCount(0);
});
