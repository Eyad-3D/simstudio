import { describe, expect, it } from "vitest";
import { giveWay } from "./energyRoom";

// on screen at 86 %: Wheel RL's box and name, Node RL's, and the figures
// under the names (the pass's picture: "lost 0.026 kWh" over "Wheel RL")
const wheelBox = { x: 1100, y: 255, w: 78, h: 46 };
const wheelName = { x: 1112, y: 304, w: 54, h: 14 };
const brakeBox = { x: 1000, y: 349, w: 78, h: 46 };
const brakeName = { x: 1013, y: 398, w: 52, h: 14 };

describe("the Energy view's figures give way to the names", () => {
  it("a figure under its own name, clear of everything, stays", () => {
    const fig = { x: 1090, y: 319, w: 100, h: 26 };
    expect(giveWay([fig], [wheelBox, wheelName, brakeBox, brakeName])).toEqual([false]);
  });

  it("a figure that would cover another part's name or box gives way, the name stays", () => {
    const overName = { x: 990, y: 390, w: 100, h: 26 };
    const overBox = { x: 1090, y: 240, w: 100, h: 26 };
    expect(giveWay([overName, overBox], [wheelBox, wheelName, brakeBox, brakeName])).toEqual([true, true]);
  });

  it("of two figures that would cover each other, the first (the bigger loss) stays", () => {
    const a = { x: 100, y: 100, w: 100, h: 26 };
    const b = { x: 150, y: 110, w: 100, h: 26 };
    const c = { x: 400, y: 100, w: 100, h: 26 };
    expect(giveWay([a, b, c], [])).toEqual([false, true, false]);
  });

  it("figures that only touch do not count as covering", () => {
    const a = { x: 0, y: 0, w: 100, h: 26 };
    const b = { x: 100, y: 0, w: 100, h: 26 };
    expect(giveWay([a, b], [])).toEqual([false, false]);
  });
});
