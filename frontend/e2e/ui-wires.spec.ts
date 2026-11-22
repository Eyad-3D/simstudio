// Wires that route themselves and ports that move: every wire on the example
// runs in horizontal and vertical stretches and never through a part; a
// port Shift+dragged to another side of its part stays there, its wire
// follows, one Ctrl+Z puts it back, and the part's right-click menu puts
// its ports back in the library's places.
import { expect, test, type Page } from "@playwright/test";
import { openApp } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

/** Each wire's straight stretches (on the screen) that are neither
 *  horizontal nor vertical, and its points that lie inside a part. */
function wireFaults(page: Page) {
  return page.evaluate(() => {
    const parts = [...document.querySelectorAll(".react-flow__node")].map((n) => n.getBoundingClientRect());
    const out: string[] = [];
    for (const path of document.querySelectorAll<SVGPathElement>(".react-flow__edge-path")) {
      const id = path.closest(".react-flow__edge")?.getAttribute("data-testid");
      // straight stretches: every L from the point before it
      const d = path.getAttribute("d") ?? "";
      let at: [number, number] | null = null;
      for (const [, cmd, args] of d.matchAll(/([MLQ])([^MLQ]*)/g)) {
        const n = args.trim().split(/[ ,]+/).map(Number);
        const to: [number, number] = [n[n.length - 2], n[n.length - 1]];
        if (cmd === "L" && at && Math.abs(to[0] - at[0]) > 0.6 && Math.abs(to[1] - at[1]) > 0.6) out.push(`${id} slants`);
        at = to;
      }
      // through a part: points along it, on the screen, inside a part's box
      const m = path.getScreenCTM()!;
      const len = path.getTotalLength();
      for (let s = 6; s < len - 6; s += 4) {
        const p = new DOMPoint(path.getPointAtLength(s).x, path.getPointAtLength(s).y).matrixTransform(m);
        if (parts.some((r) => p.x > r.left + 2 && p.x < r.right - 2 && p.y > r.top + 2 && p.y < r.bottom - 2)) {
          out.push(`${id} through a part`);
          break;
        }
      }
    }
    return out;
  });
}

/** The E-Motor's positive pin: its side and its centre on the screen. */
function motorPin(page: Page) {
  return page.evaluate(() => {
    const node = [...document.querySelectorAll(".react-flow__node")].find((n) => n.textContent?.includes("E-Motor"))!;
    const pin = node.querySelector<HTMLElement>('.react-flow__handle.source[data-handleid="pos"]')!;
    const r = pin.getBoundingClientRect();
    const side = ["left", "right", "top", "bottom"].find((s) => pin.classList.contains(`react-flow__handle-${s}`));
    return { side, x: r.x + r.width / 2, y: r.y + r.height / 2 };
  });
}

/** Where the wire into the E-Motor's positive pin ends, on the screen. */
function wireEnd(page: Page) {
  return page.evaluate(() => {
    // c-4: the HV Bus to the E-Motor's positive terminal
    const path = document.querySelector<SVGPathElement>('[data-testid="rf__edge-c-4"] .react-flow__edge-path')!;
    const m = path.getScreenCTM()!;
    const len = path.getTotalLength();
    const a = new DOMPoint(path.getPointAtLength(0).x, path.getPointAtLength(0).y).matrixTransform(m);
    const b = new DOMPoint(path.getPointAtLength(len).x, path.getPointAtLength(len).y).matrixTransform(m);
    return [a, b].map((p) => ({ x: p.x, y: p.y }));
  });
}

test("every wire runs in straight stretches round the parts", async ({ page }) => {
  await openApp(page);
  expect(await page.locator(".react-flow__edge-path").count()).toBeGreaterThan(10);
  expect(await wireFaults(page)).toEqual([]);
});

test("a port Shift+dragged to another side stays there, its wire follows, and Undo or the part's menu puts it back", async ({ page }) => {
  await openApp(page);
  const pin = await motorPin(page);
  expect(pin.side).toBe("left");
  const node = page.locator(".react-flow__node", { hasText: "E-Motor" }).first();
  const b = (await node.boundingBox())!;

  // Shift+drag the pin to the middle of the part's top edge
  await page.mouse.move(pin.x, pin.y);
  await page.keyboard.down("Shift");
  await page.mouse.down();
  await page.mouse.move(b.x + b.width / 2, b.y + 2, { steps: 8 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  await expect.poll(async () => (await motorPin(page)).side).toBe("top");
  const moved = await motorPin(page);
  // the wire now ends at the pin's new place
  await expect
    .poll(async () => Math.min(...(await wireEnd(page)).map((p) => Math.hypot(p.x - moved.x, p.y - moved.y))))
    .toBeLessThan(8);
  expect(await wireFaults(page)).toEqual([]);

  // one undo step
  await page.keyboard.press("Control+z");
  await expect.poll(async () => (await motorPin(page)).side).toBe("left");
  await expect(page.getByTitle("Undo (Ctrl+Z)")).toBeDisabled();

  // back again, then the part's menu puts the library's places back
  await page.keyboard.press("Control+y");
  await expect.poll(async () => (await motorPin(page)).side).toBe("top");
  await node.click({ button: "right" });
  await page.getByRole("button", { name: "Reset port places" }).click();
  await expect.poll(async () => (await motorPin(page)).side).toBe("left");
  // a part with no moved ports has no such item
  await node.click({ button: "right" });
  await expect(page.getByRole("button", { name: "Reset port places" })).toHaveCount(0);
});
