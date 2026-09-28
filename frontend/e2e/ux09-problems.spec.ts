// UX-09: one Problems list that is always current and points at the model.
// The model is checked by itself a moment after every change, so a problem
// shows in the list, on its part and in the status bar without pressing Data
// Checks, and clears as soon as it is fixed. A row selects the part(s) it is
// about and pans and zooms the diagram to them; a check about the model as a
// whole selects every part involved.
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonTab, showPanel } from "./app";

const node = (page: Page, label: string) =>
  page.locator(".react-flow__node").filter({ has: page.getByText(label, { exact: true }) });

/** Is the part wholly inside the diagram? */
async function inView(page: Page, label: string): Promise<boolean> {
  const n = await node(page, label).boundingBox();
  const r = (await page.locator(".react-flow").boundingBox())!;
  return Boolean(n && n.x >= r.x && n.y >= r.y && n.x + n.width <= r.x + r.width && n.y + n.height <= r.y + r.height);
}

/** Zoom to 200 % on the Wheel RR, which puts the E-Motor off screen. */
async function lookAway(page: Page): Promise<void> {
  const zoom = page.getByLabel("Zoom", { exact: true });
  await zoom.selectOption("fit");
  await page.waitForTimeout(400);
  await node(page, "Wheel RR").click();
  await page.keyboard.press(".");
  await page.waitForTimeout(400);
  await zoom.selectOption("2");
  await page.waitForTimeout(400);
  expect(await inView(page, "E-Motor"), "E-Motor off screen").toBe(false);
}

test("UX-09: problems show and clear by themselves, and a row shows its parts", async ({ page }) => {
  await openApp(page);
  const motor = node(page, "E-Motor");
  const badge = motor.getByTitle(/has no Traction Command signal/);
  const count = page.getByText(/^\d+ errors?$/);
  const row = page.getByRole("button", { name: /E-Motor.*has no Traction Command signal/ });

  // unwire the E-Motor's command; nobody presses Data Checks
  await showPanel(page, "Data Bus Connections");
  await page
    .locator(":has(> button[title='Remove connection'])", { hasText: "E-Motor · Traction Command" })
    .getByTitle("Remove connection")
    .click();
  await expect(count).toHaveText("1 error", { timeout: 2000 });
  await expect(badge).toBeVisible({ timeout: 2000 });

  // the count leads to the list, which says how to fix it
  await count.click();
  await expect(row).toBeVisible();
  await expect(row).toContainText("How to fix: In Data Bus Connections, pick a source for E-Motor · Traction Command");

  // a click, or Enter on the row, selects the part and brings it into view
  await lookAway(page);
  await row.click();
  await expect.poll(() => inView(page, "E-Motor"), { timeout: 1000 }).toBe(true);
  await expect(motor).toHaveClass(/selected/);
  await lookAway(page);
  await row.focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => inView(page, "E-Motor"), { timeout: 1000 }).toBe(true);

  // a check about the whole model names, selects and shows every part involved
  await showPanel(page, "Components");
  await page.locator("[data-component-id='vehicle.body']").dblclick();
  const vehicles = page.getByRole("button", { name: /Vehicle, Vehicle 2.*Only one Vehicle element/ });
  await vehicles.click();
  await expect(page.locator(".react-flow__node.selected")).toHaveCount(2);
  await expect.poll(async () => (await inView(page, "Vehicle")) && (await inView(page, "Vehicle 2"))).toBe(true);

  // undo both edits: the problems, badge and count clear by themselves
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(badge).toHaveCount(0, { timeout: 2000 });
  await expect(count).toHaveCount(0, { timeout: 2000 });
  await showPanel(page, "Problems");
  await expect(page.getByText("0 errors, 0 warnings", { exact: true })).toBeVisible();
});
