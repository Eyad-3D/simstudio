// UX-26: the first-steps tour (driver.js) and the step bar that ticks itself
// off. Automated browsers get neither unless asked ({ auto: true }), so the
// other tests and the screenshots see the app as before.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab } from "./app";

test("UX-26: neither the tour nor the bar shows up in an automated browser by default", async ({ page }) => {
  await openApp(page);
  await expect(page.getByRole("navigation", { name: "First steps" })).toHaveCount(0);
  await expect(page.locator(".driver-popover")).toHaveCount(0);
});

test("UX-26: a new user gets a skippable tour, and the step bar ticks itself off", async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("seeded")) {
      localStorage.setItem("lightsim-tour-v1", JSON.stringify({ auto: true }));
      sessionStorage.setItem("seeded", "1");
    }
  });
  await openApp(page);
  const tour = page.locator(".driver-popover");
  await expect(tour).toBeVisible();
  await expect(tour).toContainText("Your model");
  await tour.getByRole("button", { name: "Next" }).click();
  await expect(tour).toContainText("A part and its values");
  for (const title of ["Run", "Read the results", "Your first steps"]) {
    await tour.getByRole("button", { name: "Next" }).click();
    await expect(tour).toContainText(title);
  }
  await tour.getByRole("button", { name: "Done" }).click();
  await expect(tour).toHaveCount(0);

  const bar = page.getByRole("navigation", { name: "First steps" });
  const step = (n: number, name: string) => bar.getByRole("button", { name: new RegExp(`^Step ${n}, ${name}: `) });
  // the example has no errors and drives a cycle: Build and Choose tests are done
  await expect(step(1, "Build")).toHaveAccessibleName(/: done$/);
  await expect(step(3, "Choose tests")).toHaveAccessibleName(/: done$/);
  await expect(step(2, "Set values")).not.toHaveAccessibleName(/: done$/);
  // a value changed
  await page.locator(".react-flow__node", { hasText: "Vehicle" }).filter({ hasNotText: "Task" }).first().click();
  const mass = page.getByRole("spinbutton", { name: /Vehicle Mass/ }).first();
  await mass.fill("2000");
  await mass.press("Enter");
  await expect(step(2, "Set values")).toHaveAccessibleName(/: done$/);
  // run and read
  await step(4, "Run").click();
  await ribbonTab(page, "Results").click();
  await expect(step(4, "Run")).toHaveAccessibleName(/: done$/, { timeout: 60_000 });
  await expect(step(5, "Read results")).toHaveAccessibleName(/: done$/);
  await expect(bar).toContainText("All done");
  // progress is kept, the tour does not come back, and the bar can be hidden
  await page.reload();
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await expect(step(5, "Read results")).toHaveAccessibleName(/: done$/);
  await expect(tour).toHaveCount(0);
  await bar.getByRole("button", { name: "Hide the first steps" }).click();
  await expect(bar).toHaveCount(0);
  // the Help menu brings the tour, and the bar, back
  await page.getByRole("button", { name: "Help", exact: true }).click();
  await page.getByRole("menuitem", { name: "Show the first-steps tour" }).click();
  await expect(tour).toBeVisible();
  await expect(bar).toBeVisible();
});
