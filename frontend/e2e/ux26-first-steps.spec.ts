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
  // the run ends and Results is open: every step is done, and the bar goes by itself
  await expect(bar).toHaveCount(0, { timeout: 60_000 });
  // progress is kept, the tour does not come back, nor does the bar
  await page.reload();
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await expect(tour).toHaveCount(0);
  await expect(bar).toHaveCount(0);
  // Help shows it again, ticked, with nothing lost: the tutorial is in Help too
  const help = async (item: string) => {
    await page.getByRole("button", { name: "Help", exact: true }).click();
    await page.getByRole("menuitem", { name: item }).click();
  };
  await help("Show First steps");
  await expect(step(5, "Read results")).toHaveAccessibleName(/: done$/);
  await expect(bar).toContainText("All done");
  await page.getByRole("button", { name: "Help", exact: true }).click();
  await expect(page.getByRole("menuitem", { name: "Your first electric car (tutorial)" })).toBeVisible();
  await page.keyboard.press("Escape");
  await bar.getByRole("button", { name: "Hide the first steps" }).click();
  await expect(bar).toHaveCount(0);
  // the Help menu brings the tour, and the bar, back
  await help("Show the first-steps tour");
  await expect(tour).toBeVisible();
  await expect(bar).toBeVisible();
});

test("UX-26: a page's hint shows the first time, goes when the page is left or used, and Help shows it again", async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("seeded")) {
      localStorage.setItem("lightsim-tour-v1", JSON.stringify({ auto: true, toured: true }));
      sessionStorage.setItem("seeded", "1");
    }
  });
  await openApp(page);
  const hint = page.getByRole("status").filter({ hasText: "Results: the headline numbers" });
  await ribbonTab(page, "Results").click();
  await expect(hint).toBeVisible();
  // leaving the page takes it away, and it does not come back on a later visit
  await ribbonTab(page, "Home").click();
  await expect(hint).toHaveCount(0);
  await ribbonTab(page, "Results").click();
  await expect(page.getByRole("button", { name: "Clear history" })).toBeVisible();
  await expect(hint).toHaveCount(0);
  // Help → Show hints again: the next visit shows it, and using the page puts it away
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Help", exact: true }).click();
  await page.getByRole("menuitem", { name: "Show hints again" }).click();
  await ribbonTab(page, "Results").click();
  await expect(hint).toBeVisible();
  await page.locator("[data-results-page]").getByText(/No results yet/).click();
  await expect(hint).toHaveCount(0);
});

test("UX-26: the tour never blocks the window: a first click elsewhere closes it and still acts", async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("seeded")) {
      localStorage.setItem("lightsim-tour-v1", JSON.stringify({ auto: true }));
      sessionStorage.setItem("seeded", "1");
    }
  });
  await openApp(page);
  const tour = page.locator(".driver-popover");
  await expect(tour).toBeVisible();
  // a new user's first click goes to the ribbon, not to the tour: it used to
  // land on the tour's overlay, only close the tour, and leave every button dead
  await ribbonTab(page, "Results").click({ timeout: 5_000 });
  await expect(tour).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Clear history" })).toBeVisible();
  // and the window is clickable from then on
  await ribbonTab(page, "Home").click({ timeout: 5_000 });
  await expect(page.getByRole("button", { name: "Templates" })).toBeVisible();
});
