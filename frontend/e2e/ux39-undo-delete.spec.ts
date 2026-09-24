// UX-39: every way of deleting parts and wires is one undo step. React Flow
// reports a keyboard delete as "wires removed" and then "parts removed", and
// the canvas stored those as two edits, so one Ctrl+Z brought a part back
// without its wires. The context menu did the same with a part and a wire
// selected together.
import { expect, test, type Page } from "@playwright/test";
import { openApp, openExample, ribbonButton } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

const part = (page: Page, label: string) => page.locator(".react-flow__node", { hasText: label }).first();
const counts = (page: Page) =>
  page.evaluate(() => ({
    parts: document.querySelectorAll(".react-flow__node").length,
    wires: document.querySelectorAll(".react-flow__edge").length,
  }));

/** Ctrl+click a wire touching none of the given parts, at a point on it that
 *  no part covers. */
async function addFreeWire(page: Page, avoid: string[]): Promise<void> {
  const ids = await Promise.all(avoid.map((label) => part(page, label).getAttribute("data-id")));
  const at = await page.locator(".react-flow__edge").evaluateAll((wires, ids) => {
    for (const g of wires) {
      const label = g.getAttribute("aria-label") ?? "";
      if (ids.some((id) => label.includes(id!))) continue;
      const path = g.querySelector<SVGPathElement>(".react-flow__edge-interaction")!;
      const m = path.getScreenCTM()!;
      for (const f of [0.5, 0.4, 0.6, 0.3, 0.7]) {
        const p = path.getPointAtLength(path.getTotalLength() * f);
        const x = m.a * p.x + m.c * p.y + m.e;
        const y = m.b * p.x + m.d * p.y + m.f;
        if (document.elementFromPoint(x, y)?.closest(".react-flow__edge") === g) return { x, y };
      }
    }
    return null;
  }, ids);
  expect(at, "no free wire to click").not.toBeNull();
  await page.keyboard.down("Control");
  await page.mouse.click(at!.x, at!.y);
  await page.keyboard.up("Control");
}

/** Delete with `remove`, then check one Ctrl+Z brings every part and wire back. */
async function expectOneUndo(page: Page, remove: () => Promise<void>, gone: { parts: number; wires: number }) {
  const before = await counts(page);
  await remove();
  await expect.poll(() => counts(page)).toEqual({ parts: before.parts - gone.parts, wires: before.wires - gone.wires });
  await page.keyboard.press("Control+z");
  await expect.poll(() => counts(page)).toEqual(before);
}

test.beforeEach(async ({ page }) => {
  await openApp(page);
  await openExample(page, "Battery Electric");
});

for (const key of ["Delete", "Backspace"]) {
  test(`UX-39: ${key} on a part, then one Ctrl+Z, brings back the part and its wires`, async ({ page }) => {
    await expectOneUndo(
      page,
      async () => {
        await part(page, "Final Drive").click();
        await page.keyboard.press(key);
      },
      { parts: 1, wires: 2 },
    );
  });
}

test("UX-39: one Ctrl+Z undoes deleting a multi-selection of parts and a wire", async ({ page }) => {
  await expectOneUndo(
    page,
    async () => {
      await part(page, "Final Drive").click();
      await part(page, "Wheel RL").click({ modifiers: ["Control"] });
      await addFreeWire(page, ["Final Drive", "Wheel RL"]);
      await page.keyboard.press("Delete");
    },
    // Final Drive's 2 wires, Wheel RL's 1 and the free one
    { parts: 2, wires: 4 },
  );
});

test("UX-39: one Ctrl+Z undoes deleting only a wire", async ({ page }) => {
  await expectOneUndo(
    page,
    async () => {
      await addFreeWire(page, []);
      await page.keyboard.press("Delete");
    },
    { parts: 0, wires: 1 },
  );
});

test("UX-39: one Ctrl+Z undoes the context menu's Delete of a part and a wire", async ({ page }) => {
  await expectOneUndo(
    page,
    async () => {
      await part(page, "Final Drive").click();
      await addFreeWire(page, ["Final Drive"]);
      await part(page, "Final Drive").click({ button: "right" });
      // the menu's entry shows its shortcut, "Del"; the ribbon's Delete does not
      await page.locator("button", { hasText: /^Delete.*Del$/ }).click();
    },
    { parts: 1, wires: 3 },
  );
});

test("Delete in a map's cell clears the cell, not the part on the diagram", async ({ page }) => {
  const before = await counts(page);
  await part(page, "E-Motor").dblclick();
  await page.locator(".ss-grid .ss-cell-body").first().click();
  await page.keyboard.press("Delete");
  await page.keyboard.press("Backspace");
  // a delete would show within a frame or two of the key press
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
  expect(await counts(page)).toEqual(before);
});

test("UX-39: one Ctrl+Z undoes the ribbon's Delete", async ({ page }) => {
  await expectOneUndo(
    page,
    async () => {
      await part(page, "Final Drive").click();
      await ribbonButton(page, "Delete");
    },
    { parts: 1, wires: 2 },
  );
});
