// Shared steps for the diagram, layout and chart checks (UX-01, UX-06,
// GUI-01, RES-03, RES-04). They run under the repo's Playwright setup
// (`npm run build && npm run test:e2e`) against the real engine. Every test
// gets a fresh browser context, so the saved layout and theme start empty,
// as on a first launch.
import { expect, type Page } from "@playwright/test";

/** Load the app, wait for the example model and for the mount-time fits
 *  (the canvas re-fits 400 and 900 ms after it mounts). */
export async function openApp(page: Page): Promise<void> {
  await page.goto("/");
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await page.waitForTimeout(1200);
}

/** Current diagram zoom (the scale of React Flow's viewport). */
export function zoomOf(page: Page): Promise<number | null> {
  return page.evaluate(() => {
    const v = document.querySelector(".react-flow__viewport");
    const m = v && getComputedStyle(v).transform.match(/matrix\(([^,]+)/);
    return m ? +(+m[1]).toFixed(3) : null;
  });
}

/** The diagram's box and its share of the window, in %. */
export function canvasShare(page: Page): Promise<{ w: number; h: number; pct: number }> {
  return page.evaluate(() => {
    const r = document.querySelector(".react-flow")!.getBoundingClientRect();
    return {
      w: Math.round(r.width),
      h: Math.round(r.height),
      pct: +(((r.width * r.height) / (innerWidth * innerHeight)) * 100).toFixed(1),
    };
  });
}

/** Nodes on screen, overlapping pairs, and nodes not wholly inside the view. */
export function nodeLayout(page: Page): Promise<{ count: number; overlaps: number; offscreen: number }> {
  return page.evaluate(() => {
    const boxes = [...document.querySelectorAll(".react-flow__node")].map((n) => n.getBoundingClientRect());
    let overlaps = 0;
    for (let i = 0; i < boxes.length; i++)
      for (let j = i + 1; j < boxes.length; j++) {
        const a = boxes[i];
        const b = boxes[j];
        if (a.x < b.right && b.x < a.right && a.y < b.bottom && b.y < a.bottom) overlaps++;
      }
    const pane = document.querySelector(".react-flow")!.getBoundingClientRect();
    const offscreen = boxes.filter(
      (b) => b.x < pane.x || b.y < pane.y || b.right > pane.right || b.bottom > pane.bottom,
    ).length;
    return { count: boxes.length, overlaps, offscreen };
  });
}

/** Height of the bottom tray (34 px when collapsed to its tabs) and of the
 *  whole model workspace. */
export function trayAndDock(page: Page): Promise<{ tray: number; dock: number }> {
  return page.evaluate(() => ({
    tray: Math.round(document.querySelector(".dv-edge-group")!.getBoundingClientRect().height),
    dock: Math.round(
      document.querySelector("[aria-label='Model workspace panels']")!.getBoundingClientRect().height,
    ),
  }));
}

/** Width of each dock group, keyed by its first tab's title. */
export function groupWidths(page: Page): Promise<Record<string, number>> {
  return page.evaluate(() =>
    Object.fromEntries(
      [...document.querySelectorAll(".dv-groupview")].map((g) => [
        g.querySelector(".dv-tab")?.textContent?.trim() ?? "",
        Math.round(g.getBoundingClientRect().width),
      ]),
    ),
  );
}

/** A dock tab by its title (the accessible name may add a problem count). */
export function dockTab(page: Page, title: string) {
  return page.locator(".dv-tab", { hasText: new RegExp(`^${title}`) }).first();
}

export async function ribbonButton(page: Page, label: string): Promise<void> {
  await page.locator("button", { hasText: new RegExp(`^${label}$`) }).first().click();
}

/** Ribbon > New. */
export async function newProject(page: Page): Promise<void> {
  await ribbonButton(page, "New");
  await discardIfAsked(page);
  await expect(page.locator(".react-flow__node")).toHaveCount(0);
  await page.waitForTimeout(300);
}

/** Ribbon > Open > the example whose menu entry contains `name`. */
export async function openExample(page: Page, name: string): Promise<void> {
  await ribbonButton(page, "Open");
  await page.locator("[role=menuitem]", { hasText: name }).first().click();
  await discardIfAsked(page);
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await page.waitForTimeout(800); // the re-fit runs 120 ms later and animates for 200 ms
}

/** Answer an unsaved-work prompt (Save / Don't save / Cancel) with "Don't save". */
async function discardIfAsked(page: Page): Promise<void> {
  const discard = page.locator("button", { hasText: /^(Don.t save|Discard)/ });
  await page.waitForTimeout(300);
  if (await discard.count()) await discard.first().click();
}

/** Import a project from an in-memory file through the hidden file input. */
export async function importProject(page: Page, project: unknown): Promise<void> {
  await page
    .locator("input[type=file]")
    .first()
    .setInputFiles({ name: "project.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(project)) });
  await discardIfAsked(page);
  await page.waitForTimeout(1500);
}

/** Drop a library part on the diagram at a pane-relative point (HTML5 DnD). */
export async function dropComponent(page: Page, name: string, x: number, y: number): Promise<void> {
  const src = page.locator(".ss-tree-row[draggable=true]", { hasText: new RegExp(`^${name}`) }).first();
  const box = (await page.locator(".react-flow__pane").first().boundingBox())!;
  const dt = await page.evaluateHandle(() => new DataTransfer());
  await src.dispatchEvent("dragstart", { dataTransfer: dt });
  const target = page.locator(".react-flow").first();
  const at = { dataTransfer: dt, clientX: box.x + x, clientY: box.y + y };
  await target.dispatchEvent("dragover", at);
  await target.dispatchEvent("drop", at);
  await page.waitForTimeout(400);
}

/** Zoom the diagram out with the mouse wheel over its middle. */
export async function wheelZoomOut(page: Page, delta = 600): Promise<void> {
  const box = (await page.locator(".react-flow").first().boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, delta);
  await page.waitForTimeout(300);
}
