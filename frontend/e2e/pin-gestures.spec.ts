// Pin gestures with a real mouse: Shift+click a pin flips it to the next side
// of its node, Shift+drag moves it along the node's edge. Shift is also React
// Flow's box-select key, whose press handler used to swallow the press before
// the pin saw it; these tests drive the real pointer (not synthetic events).
import { expect, test, type Page } from "@playwright/test";
import { openApp } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

/** A pin of the E-Motor: its id, side class, and centre on screen. */
function motorPin(page: Page, id?: string) {
  return page.evaluate((id) => {
    const node = [...document.querySelectorAll(".react-flow__node")].find((n) => n.textContent?.includes("E-Motor"));
    const pin = node?.querySelector<HTMLElement>(
      id ? `.react-flow__handle.source[data-handleid="${id}"]` : ".react-flow__handle.source",
    );
    if (!pin) return null;
    const r = pin.getBoundingClientRect();
    const side = ["left", "right", "top", "bottom"].find((s) => pin.classList.contains(`react-flow__handle-${s}`));
    return { id: pin.dataset.handleid!, side, x: r.x + r.width / 2, y: r.y + r.height / 2 };
  }, id);
}

/** Press and release the mouse on (x, y) with Shift held, moving by dy in between. */
async function shiftPress(page: Page, at: { x: number; y: number }, dy = 0) {
  await page.mouse.move(at.x, at.y);
  await page.keyboard.down("Shift");
  await page.mouse.down();
  if (dy) await page.mouse.move(at.x, at.y + dy, { steps: 6 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  await page.waitForTimeout(300);
}

const selectedNodes = (page: Page) => page.locator(".react-flow__node.selected").count();

test("Shift+click a pin flips it to the next side of its node", async ({ page }) => {
  await openApp(page);
  const pin = (await motorPin(page))!;
  expect(pin, "an E-Motor pin").not.toBeNull();

  await shiftPress(page, pin);
  const flipped = (await motorPin(page, pin.id))!;
  const next = { left: "top", top: "right", right: "bottom", bottom: "left" }[pin.side!];
  expect(flipped.side, "Shift+click moved the pin to the next side").toBe(next);
  expect(await selectedNodes(page), "no box selection or node selection started").toBe(0);

  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await motorPin(page, pin.id))!.side).toBe(pin.side);
});

test("Shift+drag a pin moves it along the node's edge", async ({ page }) => {
  await openApp(page);
  const pin = (await motorPin(page))!;
  const node = page.locator(".react-flow__node", { hasText: "E-Motor" }).first();
  const nodeBefore = (await node.boundingBox())!;

  await shiftPress(page, pin, -14);
  const moved = (await motorPin(page, pin.id))!;
  expect(moved.side, "the pin stays on its side").toBe(pin.side);
  expect(pin.y - moved.y, "Shift+drag moved the pin up").toBeGreaterThanOrEqual(8);
  const nodeAfter = (await node.boundingBox())!;
  expect([nodeAfter.x, nodeAfter.y], "the node itself did not move").toEqual([nodeBefore.x, nodeBefore.y]);
  expect(await selectedNodes(page), "no box selection started").toBe(0);
});

test("Shift+drag off a pin still draws a selection box", async ({ page }) => {
  await openApp(page);
  const node = page.locator(".react-flow__node", { hasText: "E-Motor" }).first();
  const box = (await node.boundingBox())!;
  await page.mouse.move(box.x - 12, box.y - 12);
  await page.keyboard.down("Shift");
  await page.mouse.down();
  await page.mouse.move(box.x + box.width + 12, box.y + box.height + 12, { steps: 6 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  await expect(node).toHaveClass(/\bselected\b/);
});
