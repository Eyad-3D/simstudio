import { describe, expect, it } from "vitest";
import { cssZoomOf, noticeStyle, placeNotice, type Box } from "./noticePlace";

describe("the notice stays wholly in the window at every interface size", () => {
  // as drawn (window px): the small notice, 202 × 57 at 100 %, 25 % bigger at 125 %
  for (const [w, h] of [
    [1366, 768],
    [1600, 900],
    [1920, 1080],
  ] as const)
    for (const zoom of [1, 1.1, 1.25, 1.5]) {
      it(`${w}×${h} at ${Math.round(zoom * 100)} %`, () => {
        const view = { w, h };
        const size = { w: 202 * zoom, h: 57 * zoom };
        const home = { x: w - 12 - size.w, y: h - 32 - size.h };
        const at = placeNotice(size, home, [], view);
        const s = noticeStyle(at, size, view, zoom);
        // drawn at left × zoom, top × zoom (the status bar's CSS zoom)
        const x = s.left * zoom;
        const y = s.top * zoom;
        expect(x).toBeGreaterThanOrEqual(0);
        expect(y).toBeGreaterThanOrEqual(0);
        expect(x + size.w).toBeLessThanOrEqual(w);
        expect(y + size.h).toBeLessThanOrEqual(h);
        // and where it was placed, to the pixel
        expect(x).toBeCloseTo(at.x, 6);
        expect(y).toBeCloseTo(at.y, 6);
      });
    }

  it("the failure: at 125 % the place, unscaled, was off the window", () => {
    const view = { w: 1600, h: 900 };
    const size = { w: 252.5, h: 71.25 };
    const at = { x: view.w - 12 - size.w, y: view.h - 32 - size.h };
    expect(at.x * 1.25 + size.w).toBeGreaterThan(view.w); // what was drawn before
    expect(noticeStyle(at, size, view, 1.25).left * 1.25 + size.w).toBeLessThanOrEqual(view.w);
  });

  it("a place outside the window, or a window narrower than the notice, is kept inside", () => {
    const view = { w: 300, h: 200 };
    expect(noticeStyle({ x: 400, y: 300 }, { w: 202, h: 57 }, view, 1)).toEqual({ left: 98, top: 143 });
    expect(noticeStyle({ x: -20, y: -5 }, { w: 202, h: 57 }, view, 1)).toEqual({ left: 0, top: 0 });
  });

  it("reads the zoom an element is drawn at from its ancestors", () => {
    const outer = document.createElement("div");
    outer.style.zoom = "1.25";
    const inner = document.createElement("div");
    outer.append(inner);
    document.body.append(outer);
    expect(cssZoomOf(inner)).toBeCloseTo(1.25, 6);
    outer.remove();
  });
});

const view = { w: 1366, h: 768 };
const size = { w: 330, h: 74 };
const home = { x: view.w - 12 - size.w, y: view.h - 32 - size.h };
const over = (at: { x: number; y: number }, c: Box) =>
  at.x < c.x + c.w && c.x < at.x + size.w && at.y < c.y + c.h && c.y < at.y + size.h;

// at 1366 × 768 after the first run: the tray's tab strip with its Help and
// "Collapse to tabs" buttons at the right (y 709 to 731), its open panel's
// channel list and Results… button above it, the status bar's items below
const collapse = { x: 1336, y: 709, w: 22, h: 22 };
const help = { x: 1308, y: 709, w: 22, h: 22 };
const tabs = [0, 1, 2, 3, 4].map((i) => ({ x: 4 + i * 120, y: 709, w: 110, h: 26 }));
const channel = { x: 26, y: 597, w: 1210, h: 24 };
const results = { x: 1296, y: 597, w: 62, h: 24 };
const status = [
  { x: 4, y: 746, w: 110, h: 18 },
  { x: 1190, y: 746, w: 60, h: 18 },
];

describe("the run's finish notice covers no control", () => {
  it("stays in its place when nothing is under it", () => {
    expect(placeNotice(size, home, status, view)).toEqual(home);
  });

  it("moves off the tray's Collapse to tabs button (the pass's case) to the nearest clear place", () => {
    const controls = [collapse, help, ...tabs, channel, results, ...status];
    expect(over(home, collapse)).toBe(true); // where it used to be
    const at = placeNotice(size, home, controls, view);
    for (const c of controls) expect(over(at, c), JSON.stringify(c)).toBe(false);
    // up, keeping to the right edge, to just above the tab strip (over the
    // plot, which is not a control)
    expect(at.x).toBe(home.x);
    expect(at.y + size.h).toBeLessThanOrEqual(collapse.y);
    expect(at.y).toBeGreaterThanOrEqual(channel.y + channel.h);
  });

  it("goes left when going up finds no room, and never leaves the window", () => {
    const column = { x: 1100, y: 0, w: 266, h: 740 }; // a column of controls at the right
    const at = placeNotice(size, home, [column, ...status], view);
    expect(over(at, column)).toBe(false);
    expect(at.x).toBeGreaterThanOrEqual(0);
    expect(at.x + size.w).toBeLessThanOrEqual(view.w);
  });
});
