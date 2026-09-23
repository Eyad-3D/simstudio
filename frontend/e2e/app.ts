// Shared steps for the browser tests. Selectors prefer what a user sees
// (roles, labels, button text) so the tests survive styling changes.
import { expect, type Locator, type Page } from "@playwright/test";

/** Load the app and wait until the example model is drawn on the canvas. */
export async function openApp(page: Page): Promise<void> {
  await page.goto("/");
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
}

/** A ribbon tab: Project, Home, Simulations, Parameters, Results. */
export function ribbonTab(page: Page, name: string): Locator {
  return page.getByRole("button", { name, exact: true });
}

/** Bring a dock panel (Messages, Data Checks, Signal Plot, …) to the front. */
export async function showPanel(page: Page, title: string): Promise<void> {
  await page.getByRole("tab", { name: title, exact: true }).first().click();
}

/** The global Run button in the ribbon header (runs the active case). */
export function runButton(page: Page): Locator {
  return page.getByTitle(/^Run the active case/);
}

/** Run the active case and wait for the Results page to list its channels. */
export async function runActiveCase(page: Page): Promise<void> {
  await runButton(page).click();
  await expect(page.getByPlaceholder("Search channels…")).toBeVisible({ timeout: 60_000 });
}

/** The project name in the ribbon header gains a " •" while there are
 *  unsaved changes. */
export async function expectProject(
  page: Page,
  name: string,
  { unsaved }: { unsaved: boolean },
): Promise<void> {
  if (unsaved) {
    await expect(page.getByText(`${name} •`, { exact: true })).toBeVisible();
  } else {
    await expect(page.getByText(name, { exact: true }).first()).toBeVisible();
    await expect(page.getByText(`${name} •`, { exact: true })).toHaveCount(0);
  }
}

/** Log lines in the Messages panel matching `text` (shows the panel). */
export async function logLines(page: Page, text: string | RegExp): Promise<Locator> {
  await showPanel(page, "Messages");
  return page.getByText(text);
}

/** Drag a component from the library onto the canvas (the only way to add
 *  one today; UX-01 adds click/keyboard insertion). */
export async function dragComponent(page: Page, name: string, at: { x: number; y: number }) {
  await showPanel(page, "Components");
  await page.getByPlaceholder("Search components…").fill(name);
  const row = page
    .locator("[draggable=true]")
    .filter({ has: page.getByText(name, { exact: true }) })
    .first();
  await row.dragTo(page.locator(".react-flow__pane").first(), { targetPosition: at });
  await page.getByPlaceholder("Search components…").fill("");
}

/** Pick a project from the ribbon's Open menu. */
export async function openFromMenu(page: Page, name: string): Promise<void> {
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await page.getByRole("menuitem", { name: new RegExp(name) }).click();
}

/** Select an element through the Elements tree and show its Properties. */
export async function selectElement(page: Page, label: string): Promise<void> {
  await showPanel(page, "Elements");
  await page
    .locator("button.ss-tree-row")
    .filter({ has: page.getByText(label, { exact: true }) })
    .first()
    .click();
  await showPanel(page, "Properties");
}
