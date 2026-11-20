// A double-click on a part opens its parameters in a window of their own (a
// pop-up page here, a window of the desktop app there), live with the
// project: no Apply, an edit on either side shows on the other at once, one
// window per part, and the window goes when its part does.
import { expect, test, type Page } from "@playwright/test";
import { openApp, openParams, ribbonTab, selectElement } from "./app";

const torque = (p: Page) => p.getByRole("spinbutton", { name: "Generator Torque Limit Scale" });
const motor = (page: Page, label = "E-Motor") => page.locator(".react-flow__node", { hasText: label }).first();

test("a part's parameter window is live with the project, one per part, and closes with the part", async ({ page }) => {
  await openApp(page);
  await selectElement(page, "E-Motor"); // the main window's Properties show it too
  const win = await openParams(page, "E-Motor");
  await expect(win).toHaveTitle("E-Motor");
  // nothing in the main window is covered: no dialog there
  await expect(page.getByTitle("Close (Esc)")).toHaveCount(0);

  // an edit in the window is in the main window's Properties at once
  await torque(win).fill("150");
  await expect(torque(page)).toHaveValue("150");
  // and an edit in the main window is in the window at once
  await torque(page).fill("120");
  await expect(torque(win)).toHaveValue("120");

  // asking again for the part brings back the same window, never a second
  const pages = page.context().pages().length;
  await motor(page).dblclick();
  await page.waitForTimeout(500);
  expect(page.context().pages()).toHaveLength(pages);
  expect(win.isClosed()).toBe(false);

  // the window is named after the part and follows a rename
  await win.getByRole("textbox", { name: "Name", exact: true }).fill("Front Motor");
  await expect(win).toHaveTitle("Front Motor");
  await expect(motor(page, "Front Motor")).toBeVisible();

  // deleting the part closes its window
  await motor(page, "Front Motor").click();
  await page.keyboard.press("Delete");
  await expect(motor(page, "Front Motor")).toHaveCount(0);
  await expect.poll(() => win.isClosed()).toBe(true);
});

test("several parts' windows are open at once, and opening another project closes them", async ({ page }) => {
  await openApp(page);
  const a = await openParams(page, "E-Motor");
  const b = await openParams(page, "Vehicle Task");
  expect(a.isClosed() || b.isClosed()).toBe(false);
  await expect(a).toHaveTitle("E-Motor");
  await expect(b).toHaveTitle("Vehicle Task");
  // closing a window changes nothing in the project
  const dirty = await page.getByText(/ •$/).count();
  await b.close();
  await expect(page.getByText(/ •$/)).toHaveCount(dirty);

  // a new blank project (nothing was changed, so nothing to save first):
  // the old project's window goes
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("button", { name: /^Blank project/ }).click();
  await expect(page.locator(".react-flow__node", { hasText: "E-Motor" })).toHaveCount(0);
  await expect.poll(() => a.isClosed()).toBe(true);
});
