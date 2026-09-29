// Shared steps for the browser tests. Selectors prefer what a user sees
// (roles, labels, button text) so the tests survive styling changes.
import { expect, type Locator, type Page } from "@playwright/test";

/** Load the app and wait until the example model is drawn on the canvas. */
export async function openApp(page: Page): Promise<void> {
  // a new browser context opens the example as a new copy, with an id of its
  // own and so an empty run history (runs are stored per project, RES-02);
  // as a returning user who skips the Start page (ux16-start.spec.ts)
  await page.addInitScript(() => localStorage.setItem("lightsim-open-last", "1"));
  await page.goto("/");
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
}

/** A ribbon tab: Project, Home, Simulations, Parameters, Results. */
export function ribbonTab(page: Page, name: string): Locator {
  return page.getByRole("button", { name, exact: true });
}

/** Bring a dock panel (Messages, Problems, Signal Plot, …) to the front.
 *  A tab that already shows its panel is left alone, because clicking the
 *  active tab of a collapsible panel group can fold it away. The tab's name
 *  may carry a suffix such as a problem count ("Problems (2 …)"). */
export async function showPanel(page: Page, title: string): Promise<void> {
  const name = new RegExp(`^${title.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?: \\(|$)`);
  const tab = page.getByRole("tab", { name }).first();
  const showing = await tab.evaluate((el) => {
    const content = el.closest(".dv-groupview")?.querySelector(".dv-content-container");
    return Boolean(el.closest(".dv-active-tab")) && (content?.getBoundingClientRect().height ?? 0) > 0;
  });
  if (!showing) await tab.click();
}

/** Charts drawn on screen with at least one line in them. A chart is a
 *  canvas, named for screen readers by what it shows ("Results chart: SOC,
 *  …; t [s] 0 to 600"). Hidden panels (such as the model workspace kept
 *  behind the Results page) can hold charts too, so only visible ones count. */
export function drawnLines(page: Page): Locator {
  return page.getByRole("img", { name: /^(Results chart|X-Y chart|Sweep chart|Signal Plot): \S/ }).filter({ visible: true });
}

/** The x range a chart shows, read from its name ("…; t [s] 120 to 240"). */
export async function xRange(chart: Locator): Promise<[number, number]> {
  const m = /(-?[\d,.]+) to (-?[\d,.]+)$/.exec((await chart.getAttribute("aria-label")) ?? "");
  if (!m) throw new Error("the chart's name has no range");
  return [Number(m[1].replace(/,/g, "")), Number(m[2].replace(/,/g, ""))];
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

/** Pick a project from the ribbon's Open menu by its exact name (an entry
 *  also shows the project's id and description). */
export async function openFromMenu(page: Page, name: string): Promise<void> {
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await page
    .getByRole("menuitem")
    .filter({ has: page.getByText(name, { exact: true }) })
    .click();
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
