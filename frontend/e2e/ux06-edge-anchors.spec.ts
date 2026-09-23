// UX-06 keeps React Flow's measured node sizes on the nodes (the overview map
// needs them). React Flow then re-measures a node's pins only when its size
// changes, so wires must still follow a pin that is moved along its side
// (Shift+drag), flipped (Shift+click) or put back by undo.
import { expect, test, type Page } from "@playwright/test";
import { openApp, openExample } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

// distance from a point to a pin's box (wires end on the pin's outer edge)
const DIST = `(r, p) => Math.hypot(Math.max(r.x - p.x, 0, p.x - r.right), Math.max(r.y - p.y, 0, p.y - r.bottom))`;

/** Wire ends more than 2 px from every pin, and the number of wires. */
function detached(page: Page) {
  return page.evaluate((DIST) => {
    const dist = eval(DIST) as (r: DOMRect, p: DOMPoint) => number;
    const pins = [...document.querySelectorAll(".react-flow__handle")].map((h) => h.getBoundingClientRect());
    const off: [string | null | undefined, number][] = [];
    const paths = [...document.querySelectorAll<SVGPathElement>(".react-flow__edge-path")];
    for (const path of paths) {
      const m = path.getScreenCTM()!;
      for (const at of [0, path.getTotalLength()]) {
        const p = path.getPointAtLength(at).matrixTransform(m);
        const d = Math.min(...pins.map((r) => dist(r, p)));
        if (d > 2) off.push([path.closest(".react-flow__edge")?.getAttribute("data-id"), Math.round(d)]);
      }
    }
    return { wires: paths.length, off };
  }, DIST);
}

/** Centre and id of an E-Motor pin on its left or right side that has a wire. */
function connectedPin(page: Page) {
  return page.evaluate((DIST) => {
    const dist = eval(DIST) as (r: DOMRect, p: DOMPoint) => number;
    const ends: DOMPoint[] = [];
    for (const path of document.querySelectorAll<SVGPathElement>(".react-flow__edge-path")) {
      const m = path.getScreenCTM()!;
      for (const at of [0, path.getTotalLength()]) ends.push(path.getPointAtLength(at).matrixTransform(m));
    }
    const node = [...document.querySelectorAll(".react-flow__node")].find((n) => n.textContent?.includes("E-Motor"));
    const pins = node?.querySelectorAll(
      ".react-flow__handle.source.react-flow__handle-left, .react-flow__handle.source.react-flow__handle-right",
    );
    for (const h of pins ?? []) {
      const r = h.getBoundingClientRect();
      if (ends.some((p) => dist(r, p) <= 2))
        return { x: r.x + r.width / 2, y: r.y + r.height / 2, id: h.getAttribute("data-handleid")! };
    }
    return null;
  }, DIST);
}

function pinCentre(page: Page, id: string) {
  return page.evaluate((id) => {
    const node = [...document.querySelectorAll(".react-flow__node")].find((n) => n.textContent?.includes("E-Motor"));
    const r = node?.querySelector(`.react-flow__handle.source[data-handleid="${id}"]`)?.getBoundingClientRect();
    return r ? { x: r.x + r.width / 2, y: r.y + r.height / 2 } : null;
  }, id);
}

/** Shift+drag an E-Motor pin by dy (dy 0: a Shift+click). The events go to
 *  the pin directly, which keeps the pin under test exactly where it is; the
 *  same gestures with a real mouse are covered in pin-gestures.spec.ts. */
function shiftDrag(page: Page, id: string, p: { x: number; y: number }, dy: number) {
  return page.evaluate(
    async ({ id, p, dy }) => {
      const node = [...document.querySelectorAll(".react-flow__node")].find((n) => n.textContent?.includes("E-Motor"))!;
      const pin = node.querySelector(`.react-flow__handle.source[data-handleid="${id}"]`)!;
      const ev = (y: number) => ({ bubbles: true, cancelable: true, shiftKey: true, button: 0, clientX: p.x, clientY: y, view: window });
      pin.dispatchEvent(new MouseEvent("mousedown", ev(p.y)));
      for (let i = 1; dy && i <= 4; i++) {
        window.dispatchEvent(new MouseEvent("mousemove", ev(p.y + (dy * i) / 4)));
        await new Promise((r) => requestAnimationFrame(() => r(null)));
      }
      window.dispatchEvent(new MouseEvent("mouseup", ev(p.y + dy)));
    },
    { id, p, dy },
  );
}

test("UX-06: wires stay on a pin that is moved, flipped or put back by undo", async ({ page }) => {
  await openApp(page);
  expect((await detached(page)).off).toEqual([]);
  const pin = await connectedPin(page);
  expect(pin, "a connected E-Motor pin").not.toBeNull();

  await shiftDrag(page, pin!.id, pin!, -18);
  await page.waitForTimeout(500);
  const moved = (await pinCentre(page, pin!.id))!;
  expect(Math.hypot(moved.x - pin!.x, moved.y - pin!.y), "Shift+drag moved the pin").toBeGreaterThanOrEqual(8);
  expect((await detached(page)).off, "wire ends off their pin after a move").toEqual([]);

  await shiftDrag(page, pin!.id, moved, 0);
  await page.waitForTimeout(500);
  const flipped = (await pinCentre(page, pin!.id))!;
  expect(Math.hypot(flipped.x - moved.x, flipped.y - moved.y), "Shift+click flipped the pin").toBeGreaterThanOrEqual(8);
  expect((await detached(page)).off, "wire ends off their pin after a flip").toEqual([]);

  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(500);
  expect((await detached(page)).off, "wire ends off their pin after undo").toEqual([]);

  // the examples share 12 element ids; switching must re-measure the nodes
  for (const [name, wires] of [["Hybrid", 18], ["Battery Electric", 16]] as const) {
    await openExample(page, name);
    const after = await detached(page);
    expect(after.wires, `wires drawn after opening ${name}`).toBe(wires);
    expect(after.off, `wire ends off their pin after opening ${name}`).toEqual([]);
  }
});
