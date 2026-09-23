// UX-06: the diagram zoom stays readable. No jump on the first part of a
// new project, a re-fit (between 50 and 100 %) whenever a project is loaded,
// "." frames the selection, and subsystem views are remembered.
import { expect, test, type Page } from "@playwright/test";
import {
  dropComponent,
  importProject,
  newProject,
  nodeLayout,
  openApp,
  openExample,
  wheelZoomOut,
  zoomOf,
} from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

const PARTS = ["HV Battery Pack", "E-Motor", "Final Drive", "Differential", "Wheel", "Vehicle", "Driver"];

test("UX-06: dropping parts on a new project keeps the zoom at 60-100 %", async ({ page }) => {
  await openApp(page);
  await newProject(page);
  for (let i = 0; i < PARTS.length; i++) {
    await dropComponent(page, PARTS[i], 140 + (i % 4) * 170, 140 + Math.floor(i / 4) * 170);
    await expect.poll(() => zoomOf(page), { message: `zoom after dropping ${PARTS[i]}` }).toBeGreaterThanOrEqual(0.6);
    expect(await zoomOf(page)).toBeLessThanOrEqual(1);
  }
  expect(await nodeLayout(page)).toEqual({ count: PARTS.length, overlaps: 0, offscreen: 0 });
});

test("UX-06: keyboard inserts on a new project keep the zoom and do not pile up", async ({ page }) => {
  await openApp(page);
  await newProject(page);
  for (const name of PARTS) {
    await page.locator("[data-component-id]", { hasText: new RegExp(`^${name}`) }).first().focus();
    await page.keyboard.press("Enter");
    await page.waitForTimeout(250);
    const zoom = await zoomOf(page);
    expect(zoom).toBeGreaterThanOrEqual(0.6);
    expect(zoom).toBeLessThanOrEqual(1);
  }
  expect(await nodeLayout(page)).toEqual({ count: PARTS.length, overlaps: 0, offscreen: 0 });
});

test("UX-06: every project load re-fits at a readable zoom", async ({ page }) => {
  await openApp(page);
  const expectReadable = async (what: string) => {
    await expect.poll(() => zoomOf(page), { message: `zoom after ${what}` }).toBeGreaterThanOrEqual(0.5);
    expect(await zoomOf(page)).toBeLessThanOrEqual(1);
    expect((await nodeLayout(page)).offscreen, `parts outside the view after ${what}`).toBe(0);
  };
  // both examples use the root system id "sys-root"
  for (const name of ["Hybrid", "Battery Electric"]) {
    await wheelZoomOut(page);
    await openExample(page, name);
    await expectReadable(`opening ${name}`);
  }
  // opening the project that is already open (e.g. to revert it)
  await wheelZoomOut(page, 1200);
  expect(await zoomOf(page)).toBeLessThan(0.5);
  await openExample(page, "Battery Electric");
  await expectReadable("re-opening the open project");
  // importing a file with the open project's id
  await wheelZoomOut(page, 1200);
  const bev = await (await page.request.get("/api/projects/bev-car")).json();
  await importProject(page, bev);
  await expectReadable("importing the open project");
});

test("UX-06: '.' frames the selected part at up to 100 %", async ({ page }) => {
  await openApp(page);
  const node = page.locator(".react-flow__node").last();
  await wheelZoomOut(page, 800);
  await node.click();
  await page.keyboard.press(".");
  await page.waitForTimeout(700);
  const r = (await node.boundingBox())!;
  const pane = (await page.locator(".react-flow").boundingBox())!;
  expect(Math.abs(r.x + r.width / 2 - (pane.x + pane.width / 2))).toBeLessThanOrEqual(5);
  expect(Math.abs(r.y + r.height / 2 - (pane.y + pane.height / 2))).toBeLessThanOrEqual(5);
  expect(await zoomOf(page)).toBeLessThanOrEqual(1);
});

/** The BEV example copied 16 times on a 4x4 grid: 352 parts. */
async function bigModel(page: Page) {
  const bev = await (await page.request.get("/api/projects/bev-car")).json();
  const root = bev.systems[0];
  const xs = root.elements.map((e: { position: { x: number } }) => e.position.x);
  const ys = root.elements.map((e: { position: { y: number } }) => e.position.y);
  const w = Math.max(...xs) - Math.min(...xs) + 300;
  const h = Math.max(...ys) - Math.min(...ys) + 300;
  // suffix every id and every element reference
  const copy = <T extends Record<string, unknown>>(o: T, k: number): T =>
    Object.fromEntries(
      Object.entries(o).map(([key, v]) => [key, key === "id" || /elementId$|element\dId$/i.test(key) ? `${v}_${k}` : v]),
    ) as T;
  const elements = [];
  const connections = [];
  const dataBusConnections = [];
  for (let k = 0; k < 16; k++) {
    for (const e of root.elements)
      elements.push({ ...copy(e, k), position: { x: e.position.x + (k % 4) * w, y: e.position.y + Math.floor(k / 4) * h } });
    for (const c of root.connections) connections.push(copy(c, k));
    for (const d of bev.dataBusConnections ?? []) dataBusConnections.push(copy(d, k));
  }
  return {
    ...bev,
    id: "e2e-big-model",
    name: "Big model (16x BEV)",
    systems: [{ ...root, elements, connections }],
    dataBusConnections,
  };
}

test("UX-06: a model too big to read opens at 50 % with the overview map", async ({ page }) => {
  await openApp(page);
  const model = await bigModel(page);
  expect(model.systems[0].elements).toHaveLength(352);
  await importProject(page, model);
  await expect.poll(() => zoomOf(page)).toBe(0.5);
  await expect.poll(() => page.locator(".react-flow__minimap-node").count()).toBeGreaterThanOrEqual(300);
});

test("UX-06: leaving a subsystem restores the parent's view", async ({ page }) => {
  await openApp(page);
  await newProject(page);
  for (const id of ["battery.generic", "container.system"]) {
    await page.locator(`[data-component-id='${id}']`).focus();
    await page.keyboard.press("Enter");
    await page.waitForTimeout(300);
  }
  await wheelZoomOut(page, 300);
  const viewport = () =>
    page.evaluate(() => getComputedStyle(document.querySelector(".react-flow__viewport")!).transform);
  const parent = await viewport();
  await page.locator(".react-flow__node", { hasText: "System 1" }).dblclick();
  await expect.poll(() => zoomOf(page), { message: "an empty subsystem opens at 100 %" }).toBe(1);
  await page.locator(".ss-panel-toolbar button", { hasText: "New Project" }).first().click();
  await expect.poll(viewport).toBe(parent);
});
