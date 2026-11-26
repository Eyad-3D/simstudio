import { describe, expect, it } from "vitest";
import { placeNotice, type Box } from "./noticePlace";

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
