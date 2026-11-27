// The owner's calmer-screen choices B, D and G (27 Nov), with real clicks:
// B a run leaves the bottom tray as the user left it and ends with a small
// notice; D Undo, Redo and Delete only on the ribbon; G the Energy view's
// "Energy lost per part" box folded to a button that opens it. No click
// waits on a greyed item, and each wait has its own timeout.
import { expect, test, type Page } from "@playwright/test";
import { finishNotice, runActiveCase } from "./app";
import { dockTab, openApp, trayAndDock } from "./ui-helpers";

test.use({ viewport: { width: 1366, height: 768 } });

type Box = { x: number; y: number; width: number; height: number };
const overlap = (a: Box, b: Box) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

async function runFromDiagram(page: Page) {
  const pane = page.locator(".react-flow__pane").first();
  await pane.click({ position: { x: 5, y: (await pane.boundingBox())!.height - 5 } });
  await page.keyboard.press("Control+Enter");
  await expect(finishNotice(page)).toBeVisible({ timeout: 60_000 });
}

test("B: a run leaves a folded tray folded and ends with a small notice", async ({ page }) => {
  await openApp(page);
  expect((await trayAndDock(page)).tray).toBeLessThanOrEqual(36);
  await runFromDiagram(page);
  await page.waitForTimeout(800);
  expect((await trayAndDock(page)).tray, "the tray stays folded").toBeLessThanOrEqual(36);
  const notice = finishNotice(page);
  await expect(notice).toContainText(/finished: success/);
  await expect(notice.getByRole("checkbox")).toHaveCount(0);
  await expect(notice.getByRole("button")).toHaveText(["", "Show results"]); // Close (an icon), Show results
  // the plot is one click away in its tab
  await dockTab(page, "Signal Plot").click();
  await expect(page.locator("select[title='Channel to plot']")).toBeVisible({ timeout: 5000 });
});

test("B: a run leaves an open tray on the tab the user had", async ({ page }) => {
  await openApp(page);
  await dockTab(page, "Messages").click();
  await expect.poll(async () => (await trayAndDock(page)).tray, { timeout: 5000 }).toBeGreaterThanOrEqual(120);
  await runFromDiagram(page);
  await page.waitForTimeout(800);
  expect((await trayAndDock(page)).tray).toBeGreaterThanOrEqual(120);
  // still Messages in front, no Signal Plot
  await expect(page.locator("select[title='Channel to plot']")).toHaveCount(0);
  await expect(page.locator(".dv-edge-group .dv-tab.dv-active-tab", { hasText: /^Messages/ })).toHaveCount(1);
});

test("D: Undo, Redo and Delete are the ribbon's only; the keys and the right-click Delete stay", async ({ page }) => {
  await openApp(page);
  for (const title of ["Undo (Ctrl+Z)", "Redo (Ctrl+Y)", "Delete selection (Del)"])
    await expect(page.locator(`button[title='${title}']`)).toHaveCount(0);
  await expect(page.getByTitle("Zoom in")).toBeVisible();
  await expect(page.getByLabel("Zoom", { exact: true })).toBeVisible();
  await expect(page.getByTitle(/^Fit to screen/)).toBeVisible();

  const count = await page.locator(".react-flow__node").count();
  const motor = page.locator(".react-flow__node", { hasText: "E-Motor" }).first();
  // Del on the diagram, then the ribbon's Undo and Redo
  await motor.click();
  await page.keyboard.press("Delete");
  await expect(page.locator(".react-flow__node")).toHaveCount(count - 1, { timeout: 5000 });
  await page.getByRole("button", { name: "Undo", exact: true }).click({ timeout: 5000 });
  await expect(page.locator(".react-flow__node")).toHaveCount(count, { timeout: 5000 });
  await page.getByRole("button", { name: "Redo", exact: true }).click({ timeout: 5000 });
  await expect(page.locator(".react-flow__node")).toHaveCount(count - 1, { timeout: 5000 });
  await page.keyboard.press("Control+z");
  await expect(page.locator(".react-flow__node")).toHaveCount(count, { timeout: 5000 });
  // the right-click menu's Delete
  await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().click({ button: "right", position: { x: 46, y: 20 } });
  await page.locator("div.absolute.z-50").getByRole("button", { name: /^Delete/ }).click({ timeout: 5000 });
  await expect(page.locator(".react-flow__node")).toHaveCount(count - 1, { timeout: 5000 });
});

test("G: the energy box starts folded to a button, opens under the toolbar, folds again and is remembered", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  await page.getByRole("button", { name: "Home", exact: true }).click();
  await page.getByTitle(/^Energy: label each part/).click();
  const box = page.getByRole("region", { name: "Energy lost per part" });
  const button = page.getByRole("button", { name: /^Energy lost per part/ });
  await expect(box).toHaveCount(0);
  await expect(button).toHaveAttribute("aria-expanded", "false");
  // the keyboard reaches it and says what it opens
  await expect(button).toHaveAttribute("title", "Open the chart of the energy each part lost");
  await button.focus();
  await page.keyboard.press("Enter");
  await expect(box).toBeVisible({ timeout: 5000 });
  // open, it covers none of the diagram's toolbar
  const tools = (await page.getByTitle("Zoom in").locator("xpath=..").boundingBox())!;
  expect(overlap((await box.boundingBox())!, tools)).toBe(false);
  // remembered across a reload
  await page.reload();
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await page.getByTitle(/^Energy: label each part/).click();
  await expect(box).toBeVisible({ timeout: 5000 });
  // the button again folds it
  await button.click();
  await expect(box).toHaveCount(0);
  await expect(button).toHaveAttribute("aria-expanded", "false");
});
