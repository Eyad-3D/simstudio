// The diagram's defects from the app pass (fix-canvas): a copy's name, menus
// that fit, Delete on a panel tab, where Shift+A puts a part, the Energy
// figures under the names, and room for the floating name and toolbar.
// Every click on a menu item names an item that is never greyed, and each
// wait has its own timeout, so a test fails instead of hanging.
import { expect, test, type Page } from "@playwright/test";
import { ribbonTab, runActiveCase } from "./app";
import { dockTab, openApp } from "./ui-helpers";

test.use({ viewport: { width: 1366, height: 768 } });

type Box = { x: number; y: number; width: number; height: number };
const overlap = (a: Box, b: Box) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

const nodes = (page: Page) => page.locator(".react-flow__node");
const names = (page: Page) => page.locator("[data-node-name]").allInnerTexts();

test("#3: a duplicated or pasted part gets a name of its own", async ({ page }) => {
  await openApp(page);
  await nodes(page).filter({ hasText: "E-Motor" }).first().click();
  await page.keyboard.press("Control+d");
  await page.keyboard.press("Control+c");
  await page.keyboard.press("Control+v");
  await expect.poll(async () => (await names(page)).filter((n) => n.startsWith("E-Motor")).length, { timeout: 5000 }).toBe(3);
  const all = await names(page);
  expect(new Set(all).size, "every name once").toBe(all.length);
  expect(all).toEqual(expect.arrayContaining(["E-Motor", "E-Motor 2", "E-Motor 3"]));
});

test("#5: a right-click menu low on the diagram fits above the bottom panel", async ({ page }) => {
  await openApp(page);
  await dockTab(page, "Messages").click();
  await page.waitForTimeout(1000); // the tray opens and the diagram settles
  const pane = (await page.locator(".react-flow").boundingBox())!;
  // the lowest part on screen, and empty diagram near the bottom
  const boxes = await nodes(page).evaluateAll((ns) => ns.map((n) => n.getBoundingClientRect().bottom));
  const lowest = nodes(page).nth(boxes.indexOf(Math.max(...boxes.filter((b) => b < 2000))));
  for (const open of [
    () => lowest.click({ button: "right", position: { x: 20, y: 10 }, timeout: 5000 }),
    () => page.mouse.click(pane.x + 30, pane.y + pane.height - 10, { button: "right" }),
  ]) {
    await open();
    const menu = page.locator("div.absolute.z-50").filter({ has: page.getByRole("button") }).last();
    await expect(menu).toBeVisible({ timeout: 5000 });
    const m = (await menu.boundingBox())!;
    expect(m.y, "menu top").toBeGreaterThanOrEqual(pane.y);
    expect(m.y + m.height, "menu bottom above the panel").toBeLessThanOrEqual(pane.y + pane.height);
    expect(m.x + m.width, "menu right edge").toBeLessThanOrEqual(pane.x + pane.width);
    await page.keyboard.press("Escape");
  }
});

test("#6: Delete or Backspace on a focused panel tab leaves the panel open", async ({ page }) => {
  await openApp(page);
  for (const [tab, key] of [
    ["Messages", "Delete"],
    ["Problems", "Backspace"],
  ] as const) {
    await dockTab(page, tab).click();
    await page.keyboard.press(key);
    await expect(dockTab(page, tab)).toHaveCount(1, { timeout: 2000 });
  }
  // and nothing on the diagram went
  const count = await nodes(page).count();
  await dockTab(page, "Messages").click();
  await page.keyboard.press("Delete");
  await expect(nodes(page)).toHaveCount(count);
});

test("#15: Shift+A over a part puts the new part at a free place", async ({ page }) => {
  await openApp(page);
  const before = await nodes(page).count();
  // the pointer on the part nearest the top left, under the floating name
  const tops = await nodes(page).evaluateAll((ns) => ns.map((n) => n.getBoundingClientRect().top + n.getBoundingClientRect().left));
  const corner = nodes(page).nth(tops.indexOf(Math.min(...tops)));
  await corner.hover({ position: { x: 10, y: 10 }, timeout: 5000 });
  await page.keyboard.press("Shift+A");
  await page.keyboard.type("lookup");
  await page.keyboard.press("Enter");
  await expect(nodes(page)).toHaveCount(before + 1, { timeout: 5000 });
  const added = (await page.locator(".react-flow__node.selected [data-node-box]").boundingBox())!;
  for (const b of await page.locator("[data-node-box]").evaluateAll((els) =>
    els.map((e) => e.getBoundingClientRect().toJSON() as Box),
  ))
    if (b.x !== added.x || b.y !== added.y) expect(overlap(added, b), "on another part").toBe(false);
  for (const o of ["[data-diagram-name]", "button[title='Zoom in']"]) {
    const r = (await page.locator(o).first().boundingBox())!;
    expect(overlap(added, r), `under ${o}`).toBe(false);
  }
});

test("#17: the Energy figures cover no name, no part and no other figure at 50 % and 100 %", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  await ribbonTab(page, "Home").click();
  await page.getByTitle(/^Energy: label each part/).click();
  await expect(page.locator("[data-energy-label]").first()).toBeAttached({ timeout: 5000 });
  for (const zoom of ["0.5", "1"]) {
    await page.getByLabel("Zoom", { exact: true }).selectOption(zoom);
    await page.waitForTimeout(600);
    const { figures, obstacles, namesShown, namesAll } = await page.evaluate(() => {
      const box = (e: Element) => e.getBoundingClientRect().toJSON();
      const shown = [...document.querySelectorAll<HTMLElement>("[data-energy-label]")].filter(
        (e) => getComputedStyle(e).visibility !== "hidden",
      );
      const ns = [...document.querySelectorAll("[data-node-name]")];
      return {
        figures: shown.map((e) => [...e.children].map(box)),
        obstacles: [...document.querySelectorAll("[data-node-box], [data-node-name], [data-live-badge]")].map(box),
        namesShown: ns.filter((e) => getComputedStyle(e).visibility !== "hidden").length,
        namesAll: ns.length,
      };
    });
    expect(namesShown, `every name shown at ${zoom}`).toBe(namesAll);
    expect(figures.length, `some figures shown at ${zoom}`).toBeGreaterThan(0);
    const lines = figures.flat() as Box[];
    for (const l of lines) for (const o of obstacles as Box[]) expect(overlap(l, o), `a figure over a name or part at ${zoom}`).toBe(false);
    for (let i = 0; i < figures.length; i++)
      for (let j = i + 1; j < figures.length; j++)
        for (const a of figures[i] as Box[]) for (const b of figures[j] as Box[]) expect(overlap(a, b), `two figures at ${zoom}`).toBe(false);
  }
});

test("#19: the floating name lets clicks through, Fit leaves room for it and the toolbar; no React Flow link", async ({ page }) => {
  await openApp(page);
  await page.getByTitle(/^Fit to screen/).click();
  await page.waitForTimeout(600);
  const strip = (await page.locator("[data-diagram-name]").boundingBox())!;
  const tools = (await page.getByTitle("Zoom in").locator("xpath=..").boundingBox())!;
  const top = Math.max(strip.y + strip.height, tools.y + tools.height);
  const boxes = await page.locator("[data-node-box]").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().top));
  expect(Math.min(...boxes), "every part below the floating name and toolbar").toBeGreaterThanOrEqual(top);
  // a click on the name reaches the diagram under it
  const hit = await page.evaluate(({ x, y }) => !!document.elementFromPoint(x, y)?.closest("[data-diagram-name]"), {
    x: strip.x + strip.width / 2,
    y: strip.y + strip.height / 2,
  });
  expect(hit, "the name catches the click").toBe(false);
  await expect(page.locator(".react-flow__attribution")).toHaveCount(0);
});
