// The arithmetic of the table tools: shading, fill/scale/offset,
// interpolation between a selection's edges, a new breakpoint's place and
// points (also in a table of three axes), and a paste past the end.
import { describe, expect, it } from "vitest";
import { insertBreakpoint, gridKeys, type Grid } from "../tableN";
import {
  continueKeys,
  interpolateBlock,
  newBreakpoint,
  offsetBlock,
  scaleBlock,
  shadeStyle,
  viridis,
} from "./gridTools";
import { nearestCurve, pointCell, type TableSlice } from "./TablePlot";

describe("shading", () => {
  it("runs viridis from dark purple (lowest) to yellow (highest)", () => {
    expect(viridis(0)).toEqual([68, 1, 84]);
    expect(viridis(1)).toEqual([253, 231, 37]);
    expect(viridis(-3)).toEqual(viridis(0));
    // brighter all the way up: equal steps look like steps
    const light = (t: number) => viridis(t).reduce((s, v, i) => s + v * [0.2126, 0.7152, 0.0722][i], 0);
    for (let t = 0; t < 1; t += 0.05) expect(light(t + 0.05)).toBeGreaterThan(light(t));
  });
  it("tints over the cell's background, and not at all when every value is the same", () => {
    expect(shadeStyle(5, 0, 10)?.backgroundImage).toMatch(/^linear-gradient\(rgb\(\d+ \d+ \d+ \/ 0\.2\)/);
    expect(shadeStyle(5, 5, 5)).toBeUndefined();
  });
});

describe("fill, scale and offset", () => {
  it("multiply and add leave empty cells empty and write no float noise", () => {
    expect(scaleBlock([[1.1, null], [2, 3]], 3)).toEqual([[3.3, null], [6, 9]]);
    expect(offsetBlock([[0.1, null]], 0.2)).toEqual([[0.3, null]]);
  });
});

describe("interpolate between the edges", () => {
  it("a row: between its two ends, at the breakpoints' values (uneven steps)", () => {
    expect(interpolateBlock([[0, 99, 99, 30]], [0], [0, 10, 20, 30])).toEqual([[0, 10, 20, 30]]);
    expect(interpolateBlock([[0, 99, 40]], [0], [0, 30, 40])).toEqual([[0, 30, 40]]);
  });
  it("a column, and a block two wide: each line between its ends", () => {
    expect(interpolateBlock([[0, 10], [9, 9], [4, 30]], [0, 1, 2], [0, 1])).toEqual([[0, 10], [2, 20], [4, 30]]);
  });
  it("a block: the inside from all four edges, exact on a plane, the edges kept", () => {
    const f = (x: number, y: number) => 2 + 0.5 * x + 3 * y + 0.1 * x * y;
    const xs = [0, 1, 4];
    const ys = [0, 2, 3, 7];
    const block = ys.map((y, i) => xs.map((x, j) => (i === 0 || i === 3 || j === 0 || j === 2 ? f(x, y) : -1)));
    const out = interpolateBlock(block, ys, xs) as number[][];
    ys.forEach((y, i) => xs.forEach((x, j) => expect(out[i][j]).toBeCloseTo(f(x, y), 10)));
  });
  it("says why when it cannot", () => {
    expect(interpolateBlock([[1, 2], [3, 4]], [0, 1], [0, 1])).toMatch(/at least three cells/);
    expect(interpolateBlock([[null, 2, 3]], [0], [0, 1, 2])).toMatch(/empty cell/);
  });
});

describe("a new breakpoint", () => {
  it("sits midway between its neighbours, or past an end at the last step", () => {
    const keys = ["0", "1000", "3000"];
    expect(newBreakpoint(keys, 1, "before")).toBe("500");
    expect(newBreakpoint(keys, 1, "after")).toBe("2000");
    expect(newBreakpoint(keys, 0, "before")).toBe("-1000");
    expect(newBreakpoint(keys, 2, "after")).toBe("5000");
    expect(newBreakpoint(["0.1", "0.2"], 0, "after")).toBe("0.15");
    expect(newBreakpoint(["7"], 0, "after")).toBe("8");
  });
  it("keeps a curve the same: interpolated between neighbours, a copy of the edge past an end", () => {
    const t: Grid = { "0": 10, "100": 30 };
    expect(insertBreakpoint(t, 0, "25")).toEqual({ "0": 10, "25": 15, "100": 30 });
    expect(insertBreakpoint(t, 0, "200")).toEqual({ "0": 10, "100": 30, "200": 30 });
    expect(insertBreakpoint(t, 0, "100")).toBe(t);
  });
  it("in a table of three axes adds the whole slice along every other axis", () => {
    // Temperature × SOC × Current
    const t: Grid = {
      "0": { "20": { "-100": 3.1, "100": 2.9 }, "80": { "-100": 2.6, "100": 2.4 } },
      "25": { "20": { "-100": 1.9, "100": 1.8 }, "80": { "-100": 1.5, "100": 1.4 } },
    };
    const out = insertBreakpoint(t, 1, "50") as Record<string, Record<string, Record<string, number>>>;
    expect(gridKeys(out, 3)).toEqual([["0", "25"], ["20", "50", "80"], ["-100", "100"]]);
    expect(out["0"]["50"]).toEqual({ "-100": 2.85, "100": 2.65 });
    expect(out["25"]["50"]).toEqual({ "-100": 1.7, "100": 1.6 });
    // the first axis, past its end: every SOC and Current copied from 25 °C
    const hot = insertBreakpoint(t, 0, "50") as Record<string, unknown>;
    expect(hot["50"]).toEqual(t["25"]);
    // the last axis, midway
    expect((insertBreakpoint(t, 2, "0") as typeof out)["25"]["80"]).toEqual({ "-100": 1.5, "0": 1.45, "100": 1.4 });
  });
  it("in a map with gaps: a point missing on one side copies the other", () => {
    const t: Grid = { "0": { "0": 1, "10": 2 }, "10": { "0": 3 } };
    expect(insertBreakpoint(t, 0, "5")).toEqual({ "0": { "0": 1, "10": 2 }, "5": { "0": 2, "10": 2 }, "10": { "0": 3 } });
  });
});

describe("a paste past the end", () => {
  it("numbers the breakpoints it did not give on from the last step", () => {
    expect(continueKeys(["0", "500", "1000", "", ""])).toEqual(["0", "500", "1000", "1500", "2000"]);
    expect(continueKeys(["3", ""])).toEqual(["3", "4"]);
    expect(continueKeys(["0", "500", "750", ""])).toEqual(["0", "500", "750", "1000"]);
  });
});

describe("a point clicked on the plot", () => {
  const slice: TableSlice = {
    x: { title: "x", values: [0, 1] },
    yTitle: "y",
    yUnit: "",
    curves: [{ label: "a", y: [0, 10] }, { label: "b", y: [5, null] }, { label: "c", y: [9, 20] }],
    mark: null,
  };
  it("is the curve nearest to the pointer at that x, skipping gaps", () => {
    expect(nearestCurve(slice, 0, 6)).toBe(1);
    expect(nearestCurve(slice, 1, 12)).toBe(0);
  });
  it("is the cell at its index on each axis, the held axes at their breakpoint", () => {
    // three axes, x along the last, a curve per breakpoint of the middle one, the first held at 1
    expect(pointCell(3, 2, 4, 7, [1, 0, 0])).toEqual([1, 4, 7]);
    expect(pointCell(1, 0, 0, 3)).toEqual([3]);
  });
});
