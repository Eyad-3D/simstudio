import { describe, expect, it } from "vitest";
import { placeMenu } from "./menuPlace";

// the diagram at 1366×768 with the bottom panel open: 918 × 420 px
const area = { width: 918, height: 420 };
const menu = { width: 176, height: 257 }; // a part's menu with Show hidden ports
const inside = (p: { left: number; top: number; maxHeight?: number }, m = menu) =>
  p.left >= 0 && p.top >= 0 && p.left + m.width <= area.width && p.top + (p.maxHeight ?? m.height) <= area.height;

describe("a right-click menu fits inside the diagram", () => {
  it("opens below and right of the pointer when there is room", () => {
    expect(placeMenu(100, 50, menu, area)).toEqual({ left: 100, top: 50 });
  });

  it("flips above the pointer near the bottom (the part low in the diagram)", () => {
    // the pass's case: opened at y 220 of 420, it ran 37 px under the panel
    const p = placeMenu(30, 300, menu, area);
    expect(p.top).toBe(300 - 257);
    expect(inside(p)).toBe(true);
  });

  it("moves up to the edge when it fits neither below nor above", () => {
    const p = placeMenu(30, 200, menu, area);
    expect(inside(p)).toBe(true);
    expect(p.top + menu.height).toBe(area.height - 4);
  });

  it("flips left near the right edge", () => {
    const p = placeMenu(880, 20, menu, area);
    expect(p.left).toBe(880 - 176);
    expect(inside(p)).toBe(true);
  });

  it("a menu taller than the diagram starts at its top and scrolls", () => {
    const tall = { width: 176, height: 600 };
    const p = placeMenu(30, 100, tall, area);
    expect(p).toEqual({ left: 30, top: 4, maxHeight: 412 });
  });

  it("is inside for every point of the diagram", () => {
    for (let x = 0; x <= area.width; x += 17)
      for (let y = 0; y <= area.height; y += 13) expect(inside(placeMenu(x, y, menu, area))).toBe(true);
  });
});
