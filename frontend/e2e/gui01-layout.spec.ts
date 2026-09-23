// GUI-01: the default layout gives the diagram most of the window. The side
// columns take a share of the width, and the log, checks, layers, data bus
// and signal plot share a bottom tray that starts collapsed to its tabs and,
// when open, takes at most about a third of the workspace.
import { expect, test, type Page } from "@playwright/test";
import {
  canvasShare,
  dockTab,
  dropComponent,
  groupWidths,
  newProject,
  nodeLayout,
  openApp,
  ribbonButton,
  trayAndDock,
  zoomOf,
} from "./ui-helpers";

// the desktop window's minimum size, a common laptop, the window's default
// size and a full-HD screen; the targets come from the GUI-01 metric
const SIZES = [
  { width: 1024, height: 700, minShare: 35 },
  { width: 1366, height: 768, minShare: 45 },
  { width: 1600, height: 1000, minShare: 50 },
  { width: 1920, height: 1080, minShare: 55 },
];

for (const { width, height, minShare } of SIZES) {
  test.describe(`${width}x${height}`, () => {
    test.use({ viewport: { width, height } });

    test(`GUI-01: the diagram gets at least ${minShare} % of a fresh window`, async ({ page }) => {
      await openApp(page);
      expect((await canvasShare(page)).pct).toBeGreaterThanOrEqual(minShare);
      expect((await trayAndDock(page)).tray, "the tray starts collapsed").toBeLessThanOrEqual(36);
      if (width === 1366) expect(await zoomOf(page), "BEV fit zoom").toBeGreaterThanOrEqual(0.4);
    });
  });
}

test.describe("1600x1000", () => {
  test.use({ viewport: { width: 1600, height: 1000 } });

  test("GUI-01: double-clicking the Topology tab maximises the diagram and back", async ({ page }) => {
    await openApp(page);
    const start = await canvasShare(page);
    await dockTab(page, "Topology").dblclick();
    await expect.poll(async () => (await canvasShare(page)).pct).toBeGreaterThan(start.pct + 10);
    await dockTab(page, "Topology").dblclick();
    await expect.poll(async () => (await canvasShare(page)).pct).toBeCloseTo(start.pct, 0);
  });
});

test.describe("1366x768", () => {
  test.use({ viewport: { width: 1366, height: 768 } });

  test("GUI-01: Data Checks opens the tray, which stays collapsed across a reload", async ({ page }) => {
    await openApp(page);
    const start = await canvasShare(page);
    await ribbonButton(page, "Simulations");
    await ribbonButton(page, "Checks");
    await expect.poll(async () => (await trayAndDock(page)).tray).toBeGreaterThanOrEqual(120);
    await page.locator(".dv-edge-group button[aria-label='Collapse to tabs']").click();
    await expect.poll(async () => (await trayAndDock(page)).tray).toBeLessThanOrEqual(36);
    await page.waitForTimeout(700); // the layout is saved 500 ms after a change
    await page.reload();
    await expect(page.locator(".react-flow__node").first()).toBeVisible();
    await page.waitForTimeout(1200);
    expect((await trayAndDock(page)).tray).toBeLessThanOrEqual(36);
    expect((await canvasShare(page)).pct).toBeCloseTo(start.pct, 0);
  });

  test("GUI-01: Messages and Data Checks tabs carry a problem count", async ({ page }) => {
    await openApp(page);
    await newProject(page);
    await dropComponent(page, "E-Motor", 300, 200); // unwired: Data Checks finds problems
    await ribbonButton(page, "Simulations");
    await ribbonButton(page, "Checks");
    await expect(page.locator(".dv-default-tab[data-badge][aria-label^='Messages (']")).toHaveCount(1);
    await expect(page.locator(".dv-default-tab[data-badge][aria-label^='Data Checks (']")).toHaveCount(1);
  });

  test("GUI-01: the first run's Signal Plot does not cut the model off", async ({ page }) => {
    await openApp(page);
    expect((await nodeLayout(page)).offscreen).toBe(0);
    await page.locator(".react-flow__pane").first().click({ position: { x: 5, y: 5 } });
    await page.keyboard.press("Control+Enter");
    await expect(page.locator(".ss-zoom.absolute .recharts-line-curve").first()).toBeAttached({ timeout: 60_000 });
    await ribbonButton(page, "Home");
    // the tray opened on the Signal Plot by itself; the diagram re-fitted
    expect((await trayAndDock(page)).tray).toBeGreaterThanOrEqual(120);
    await expect.poll(async () => (await nodeLayout(page)).offscreen, { message: "parts cut off" }).toBe(0);
    expect(await zoomOf(page)).toBeGreaterThanOrEqual(0.5);
  });

  test("GUI-01: a maximised diagram is not what a reload brings back", async ({ page }) => {
    await openApp(page);
    const start = await groupWidths(page);
    await dockTab(page, "Topology").dblclick();
    await expect.poll(async () => (await groupWidths(page)).Components).toBe(0);
    await page.waitForTimeout(900); // past the 500 ms save delay
    await page.reload();
    await expect(page.locator(".react-flow__node").first()).toBeVisible();
    await page.waitForTimeout(1200);
    expect(await groupWidths(page)).toEqual(start);
  });
});

test.describe("1920x1080", () => {
  test.use({ viewport: { width: 1920, height: 1080 } });

  const trayShare = async (page: Page) => {
    const { tray, dock } = await trayAndDock(page);
    return tray / dock;
  };

  test("GUI-01: an open tray keeps to about a third of a shrinking window", async ({ page }) => {
    await openApp(page);
    await dockTab(page, "Messages").click();
    await expect.poll(async () => (await trayAndDock(page)).tray).toBeGreaterThanOrEqual(120);
    expect(await trayShare(page)).toBeLessThanOrEqual(0.36);
    // the window shrinks to the desktop app's minimum size
    await page.setViewportSize({ width: 1024, height: 700 });
    await expect.poll(() => trayShare(page)).toBeLessThanOrEqual(0.36);
    expect((await canvasShare(page)).pct).toBeGreaterThanOrEqual(25);
  });

  test("GUI-01: a tray saved open on a big screen fits a smaller one", async ({ page, context }) => {
    await openApp(page);
    await dockTab(page, "Messages").click();
    await expect.poll(async () => (await trayAndDock(page)).tray).toBeGreaterThanOrEqual(250);
    await page.waitForTimeout(900); // past the 500 ms save delay
    const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("lightsim-layout-v1")!));
    expect(saved.layout.edgeGroups.bottom.size, "saved tray height").toBeGreaterThanOrEqual(250);
    await page.close();
    // a new window on the same profile, on a 1366x768 screen
    const small = await context.newPage();
    await small.setViewportSize({ width: 1366, height: 768 });
    await openApp(small);
    expect((await trayAndDock(small)).tray, "the tray was saved open").toBeGreaterThanOrEqual(120);
    await expect.poll(() => trayShare(small)).toBeLessThanOrEqual(0.36);
  });
});
