import { expect, test } from "vitest";
import {
  addAxis,
  addBreakpoint,
  dropAxis,
  filled,
  getAt,
  gridKeys,
  gridValues,
  removeBreakpoint,
  renameBreakpoint,
  setAt,
  unitGrid,
  type Grid,
} from "./tableN";
import { mapTable, tableNumbers } from "./units";

/** A full grid of `depth` axes, breakpoints 0 and 10·(axis+1); a value is
 *  the sum of its breakpoints. */
function grid(depth: number): Grid {
  const level = (d: number, acc: number): Grid =>
    d === depth ? acc : Object.fromEntries([0, 10 * (d + 1)].map((k) => [String(k), level(d + 1, acc + k)]));
  return level(0, 0);
}

test.each([1, 2, 3, 4])("depth %i: the breakpoints of each axis, and the values last axis fastest", (depth) => {
  const t = grid(depth);
  expect(gridKeys(t, depth)).toEqual(Array.from({ length: depth }, (_, d) => ["0", String(10 * (d + 1))]));
  const values = gridValues(t);
  expect(values).toHaveLength(2 ** depth);
  expect(values[1]).toBe(10 * depth); // the second point differs on the last axis
  // a project file keeps it as it is
  expect(JSON.parse(JSON.stringify(t))).toEqual(t);
});

test.each([1, 2, 3, 4])("depth %i: a breakpoint added on any axis copies its nearest, in every slice", (depth) => {
  for (let axis = 0; axis < depth; axis++) {
    const near = String(10 * (axis + 1));
    const t = addBreakpoint(grid(depth), axis, String(10 * (axis + 1) + 1));
    const keys = gridKeys(t, depth);
    expect(keys[axis]).toEqual(["0", near, String(10 * (axis + 1) + 1)]);
    expect(gridValues(t)).toHaveLength(3 * 2 ** (depth - 1));
    // every point of the new slice is its nearest breakpoint's
    const at = (k: string) => keys.map((ks, a) => (a === axis ? k : ks[ks.length - 1]));
    expect(getAt(t, at(String(10 * (axis + 1) + 1)))).toBe(getAt(t, at(near)));
    // and it goes again, the grid as it was
    expect(removeBreakpoint(t, axis, String(10 * (axis + 1) + 1))).toEqual(grid(depth));
  }
});

test("a breakpoint moved keeps its slice; a cell set at a path changes that point only", () => {
  const t = renameBreakpoint(grid(3), 1, "20", "25");
  expect(gridKeys(t, 3)[1]).toEqual(["0", "25"]);
  expect(getAt(t, ["10", "25", "30"])).toBe(60);
  const u = setAt(grid(3), ["10", "0", "30"], 99);
  expect(getAt(u, ["10", "0", "30"])).toBe(99);
  expect(gridValues(u).filter((v, i) => v !== gridValues(grid(3))[i])).toEqual([99]);
});

test("an axis added holds the values as its one slice; one dropped keeps the slice chosen", () => {
  expect(getAt(addAxis(grid(2)), ["10", "20", "0"])).toBe(30);
  expect(addAxis(5)).toEqual({ "0": 5 });
  expect(dropAxis(grid(3), 1, "20")).toEqual(
    Object.fromEntries(["0", "10"].map((a) => [a, { "0": Number(a) + 20, "30": Number(a) + 50 }])),
  );
  expect(dropAxis({ "0": 1, "5": 2 }, 0, "5")).toBe(2);
  expect(filled(grid(2), 0)).toEqual({ "0": { "0": 0, "20": 0 }, "10": { "0": 0, "20": 0 } });
  expect(unitGrid(3)).toEqual({ "0": { "0": { "0": 0 } } });
  expect(unitGrid(0, 4)).toBe(4);
});

test("units at any depth: an axis's breakpoints, or the values, converted", () => {
  const t = grid(4);
  const kw = mapTable(t, "value", (v) => v / 1000);
  expect(getAt(kw, ["10", "20", "30", "40"])).toBe(0.1);
  const third = mapTable(t, 2, (k) => k * 2);
  expect(gridKeys(third, 4)[2]).toEqual(["0", "60"]);
  expect(tableNumbers(t, 3)).toHaveLength(16);
});
