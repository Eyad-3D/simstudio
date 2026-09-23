// RES-04: nothing from the Home dock hidden behind the Results page (its
// splitters in particular) may lie on top of Results, dragging where a
// splitter would be must not resize hidden panels, and the dock must fill a
// window resized while Results was shown.
import { expect, test } from "@playwright/test";
import { openApp, ribbonButton } from "./ui-helpers";

const DOCK = "[aria-label='Model workspace panels']";

for (const [width, height] of [
  [1280, 800],
  [1600, 900],
  [1920, 1080],
] as const) {
  test.describe(`${width}x${height}`, () => {
    test.use({ viewport: { width, height } });

    test("RES-04: the hidden dock stays out of the Results page", async ({ page }) => {
      await openApp(page);
      const groupRects = () =>
        page.locator(`${DOCK} .dv-groupview`).evaluateAll((groups) =>
          groups.map((g) => {
            const r = g.getBoundingClientRect();
            return [r.x, r.y, r.width, r.height].map(Math.round).join(",");
          }),
        );
      const sashes = await page.locator(".dv-sash").evaluateAll((els) =>
        els
          .map((el) => el.getBoundingClientRect())
          .filter((r) => r.width > 0 && r.height > 0)
          .map((r) => ({ x: r.x + r.width / 2, y: r.y + r.height / 2, vertical: r.height > r.width })),
      );
      expect(sashes.length).toBeGreaterThan(0);
      const before = await groupRects();

      await ribbonButton(page, "Results");
      await page.waitForTimeout(500);
      // sample the Results page on an 8 px grid: nothing of the dock on top
      const hits = await page.evaluate((DOCK) => {
        const dock = document.querySelector(DOCK)!;
        const bad: string[] = [];
        for (let y = 110; y < innerHeight - 30; y += 8)
          for (let x = 4; x < innerWidth - 4; x += 8) {
            const top = document.elementFromPoint(x, y);
            if (top && (top.classList.contains("dv-sash") || dock.contains(top))) bad.push(`${x},${y}`);
          }
        return bad;
      }, DOCK);
      expect(hits.slice(0, 5), `${hits.length} points on Results hit the hidden dock`).toEqual([]);

      // drag across every splitter position while on Results
      for (const s of sashes) {
        await page.mouse.move(s.x, s.y);
        await page.mouse.down();
        await page.mouse.move(s.vertical ? s.x + 80 : s.x, s.vertical ? s.y : s.y - 80, { steps: 4 });
        await page.mouse.up();
      }
      await ribbonButton(page, "Home");
      await page.waitForTimeout(500);
      expect(await groupRects(), "hidden panels resized by dragging on Results").toEqual(before);

      // resize the window on Results; back on Home the dock fills the window
      await ribbonButton(page, "Results");
      await page.setViewportSize({ width: width - 200, height: height - 100 });
      await page.waitForTimeout(500);
      await ribbonButton(page, "Home");
      await page.waitForTimeout(700);
      const gaps = await page.evaluate((DOCK) => {
        const dock = document.querySelector(DOCK)!;
        const box = dock.getBoundingClientRect();
        const groups = [...dock.querySelectorAll(".dv-groupview")].map((g) => g.getBoundingClientRect());
        return {
          right: Math.round(box.right - Math.max(...groups.map((g) => g.right))),
          bottom: Math.round(box.bottom - Math.max(...groups.map((g) => g.bottom))),
        };
      }, DOCK);
      expect(Math.abs(gaps.right)).toBeLessThanOrEqual(2);
      expect(Math.abs(gaps.bottom)).toBeLessThanOrEqual(2);
    });
  });
}
