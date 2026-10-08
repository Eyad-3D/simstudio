// CON-33: the Formula Student example opens from the Open menu on its 75 m
// acceleration case, and one Run gives the 75 m time, held at the 80 kW
// power limit, within 5 s.
import { expect, test } from "@playwright/test";
import { finishNotice, headlineTile, openApp, openFromMenu, openSummary, runButton, showResults } from "./app";

test("CON-33: the FS example runs its 75 m test from the Open menu within 5 s", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await expect(page.locator(".react-flow__node", { hasText: "Accumulator" })).toBeVisible();
  await expect(page.getByTitle("Active simulation case")).toHaveValue("case-accel-75m");

  const t0 = Date.now();
  await runButton(page).click();
  // the run has ended when its notice shows (UX-21)
  await expect(finishNotice(page)).toBeVisible({ timeout: 5000 });
  expect(Date.now() - t0).toBeLessThan(5000);
  await showResults(page);
  const time = headlineTile(page, "Time to 75 m");
  await expect(time).toContainText("pass");
  // the pass chip is the 25 s limit: the time itself is 3.5-4.5 s
  await expect(time).toContainText(/^Time to 75 m(3\.[5-9]|4\.[0-4])/);
  await expect(time).toContainText("≤ 25");
  await openSummary(page);
  await expect(page.getByRole("row", { name: /^Accumulator — peak terminal power, averaged/ })).toContainText(
    "pass",
  );
  await expect(page.getByText(/Cycle not followed/)).toHaveCount(0);
});
