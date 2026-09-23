// GUI-33: the ribbon's Open and Restore… menus, and the diagram's right-click
// menu, close on Esc and on a click anywhere outside them, the diagram
// included (React Flow stops the mousedown the menus used to listen for).
// Esc gives the focus back to the button that opened the menu.
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonTab } from "./app";

/** Click empty diagram space, clear of the menus on the left and of the
 *  attribution in the bottom-right corner. */
async function clickDiagram(page: Page): Promise<void> {
  const pane = page.locator(".react-flow__pane").first();
  const box = (await pane.boundingBox())!;
  await pane.click({ position: { x: box.width - 20, y: box.height / 2 } });
}

for (const { tab, name, menu } of [
  { tab: "Home", name: "Open", menu: "Open project" },
  { tab: "Project", name: "Restore…", menu: "Earlier versions" },
]) {
  test(`GUI-33: the ${name} menu closes on a click on the diagram, on another panel and on Esc`, async ({ page }) => {
    await openApp(page);
    await ribbonTab(page, tab).click();
    const button = page.getByRole("button", { name, exact: true });
    const list = page.getByRole("menu", { name: menu });

    await button.click();
    await expect(list).toBeVisible();
    await clickDiagram(page);
    await expect(list).toHaveCount(0);

    await button.click();
    await expect(list).toBeVisible();
    await page.getByRole("tab", { name: /^Messages/ }).first().click();
    await expect(list).toHaveCount(0);

    await button.click();
    await expect(list).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(list).toHaveCount(0);
    await expect(button).toBeFocused();
  });
}

test("GUI-33: the Open menu is gone when a double-click opens the parameter dialog", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("menu", { name: "Open project" })).toBeVisible();
  await page.locator(".react-flow__node").last().dblclick();
  await expect(page.getByTitle("Close (Esc)")).toBeVisible();
  await expect(page.getByRole("menu", { name: "Open project" })).toHaveCount(0);
});

test("GUI-33: the diagram's right-click menu closes on a click outside the diagram", async ({ page }) => {
  await openApp(page);
  const pane = page.locator(".react-flow__pane").first();
  const box = (await pane.boundingBox())!;
  await pane.click({ button: "right", position: { x: box.width - 220, y: 40 } });
  const selectAll = page.getByRole("button", { name: "Select all" });
  await expect(selectAll).toBeVisible();
  await ribbonTab(page, "Home").click();
  await expect(selectAll).toHaveCount(0);
});
