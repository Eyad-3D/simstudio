// UX-27: a panel closed with its tab's X comes back when a button asks for
// it. Before, every button that shows it did nothing (Simulations →
// Settings… with Cases & Parameters closed), even after a restart, because
// the layout is saved without the panel.
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonTab } from "./app";

const tab = (page: Page, name: string) => page.getByRole("tab", { name: new RegExp(`^${name}`) });

async function close(page: Page, name: string) {
  await tab(page, name).click();
  await tab(page, name).locator(".dv-default-tab-action").click();
  await expect(tab(page, name)).toHaveCount(0);
}

test("UX-27: a closed panel comes back when a button asks for it, after a restart too", async ({ page }) => {
  await openApp(page);
  // the whole right column: Cases & Parameters, then Properties
  await close(page, "Cases & Parameters");
  await close(page, "Properties");
  await ribbonTab(page, "Simulations").click();
  const settings = page.getByTitle(/^Edit duration, step/);
  await settings.click();
  await expect(tab(page, "Cases & Parameters")).toBeVisible();
  await expect(tab(page, "Cases & Parameters")).toHaveClass(/dv-active-tab/);
  await expect(page.getByText("Case settings", { exact: true })).toBeVisible();
  // back in a column of its own on the right of the diagram
  const diagram = (await tab(page, "Topology").boundingBox())!;
  expect((await tab(page, "Cases & Parameters").boundingBox())!.x).toBeGreaterThan(diagram.x + diagram.width);

  // closed again and the app restarted: the saved layout has no Cases panel
  await close(page, "Cases & Parameters");
  await page.waitForTimeout(800); // the layout is saved half a second after a change
  await page.reload();
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await expect(tab(page, "Cases & Parameters")).toHaveCount(0);
  await ribbonTab(page, "Simulations").click();
  await settings.click();
  await expect(tab(page, "Cases & Parameters")).toBeVisible();

  // Workspace → Properties brings Properties back beside it
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Properties", exact: true }).click();
  await expect(tab(page, "Properties")).toBeVisible();
});

test("UX-27: a closed tray panel comes back too", async ({ page }) => {
  await openApp(page);
  await close(page, "Problems");
  // Simulations → Checks runs the Data Checks and shows their list
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "Checks", exact: true }).click();
  await expect(tab(page, "Problems")).toBeVisible();
  await expect(page.getByRole("button", { name: "Run Data Checks" })).toBeVisible();
});
