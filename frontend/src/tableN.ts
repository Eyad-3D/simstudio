// Tables of any number of axes as full grids (docs/spec/tables.md): nested
// {breakpoint: …} objects, one level per axis, numbers at the bottom. These
// helpers never change a table in place; each returns a new one.
import type { TableN } from "./types";

/** A table, or (no axes) a single number. */
export type Grid = TableN | number;

const byNumber = (a: string, b: string) => Number(a) - Number(b);

/** The breakpoints of each of a full grid's `depth` axes, in increasing
 *  order, as the keys are written (read along its first entries). */
export function gridKeys(t: Grid, depth: number): string[][] {
  const out: string[][] = [];
  let node: Grid | undefined = t;
  for (let d = 0; d < depth; d++) {
    const keys: string[] = node && typeof node === "object" ? Object.keys(node).sort(byNumber) : [];
    out.push(keys);
    node = node && typeof node === "object" && keys.length ? node[keys[0]] : undefined;
  }
  return out;
}

/** The entry at one breakpoint of each of the first `path.length` axes. */
export function getAt(t: Grid, path: string[]): Grid | undefined {
  let node: Grid | undefined = t;
  for (const k of path) node = node && typeof node === "object" ? node[k] : undefined;
  return node;
}

/** The table with the entry at `path` set to `v`. */
export function setAt(t: Grid, path: string[], v: Grid): Grid {
  if (path.length === 0) return v;
  const node = (typeof t === "object" ? t : {}) as TableN;
  const [k, ...rest] = path;
  return { ...node, [k]: setAt(node[k] ?? {}, rest, v) };
}

/** `fn` applied to every object at level `axis` (axis 0: the table itself). */
function atLevel(t: Grid, axis: number, fn: (node: TableN) => TableN): Grid {
  if (typeof t !== "object") return t;
  if (axis === 0) return fn(t);
  const out: TableN = {};
  for (const [k, v] of Object.entries(t)) out[k] = atLevel(v, axis - 1, fn);
  return out;
}

/** The breakpoint of `keys` nearest to `n` (the lower one on a tie). */
export function nearest(keys: string[], n: number): string {
  return keys.reduce((best, k) => (Math.abs(Number(k) - n) < Math.abs(Number(best) - n) ? k : best), keys[0]);
}

/** A new breakpoint `key` on axis `axis`: its points copy those of the
 *  nearest breakpoint there, in every slice. */
export function addBreakpoint(t: Grid, axis: number, key: string): Grid {
  return atLevel(t, axis, (node) => {
    const keys = Object.keys(node);
    if (keys.some((k) => Number(k) === Number(key)) || keys.length === 0) return node;
    return { ...node, [key]: structuredClone(node[nearest(keys, Number(key))]) };
  });
}

/** (1 − w)·a + w·b, entry by entry; where one side has no entry, the other's. */
function blend(a: Grid | undefined, b: Grid | undefined, w: number): Grid | undefined {
  if (a === undefined || b === undefined) return structuredClone(a ?? b);
  if (typeof a === "number" && typeof b === "number") return a === b ? a : Number((a + (b - a) * w).toPrecision(12));
  if (typeof a !== "object" || typeof b !== "object") return structuredClone(a);
  const out: TableN = {};
  for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) {
    const v = blend(a[k], b[k], w);
    if (v !== undefined) out[k] = v;
  }
  return out;
}

/** A new breakpoint `key` on axis `axis` that leaves the table meaning the
 *  same: its points are interpolated linearly between its neighbours, or
 *  past an end copy the edge's, in every slice. */
export function insertBreakpoint(t: Grid, axis: number, key: string): Grid {
  const n = Number(key);
  return atLevel(t, axis, (node) => {
    const keys = Object.keys(node).sort(byNumber);
    if (keys.length === 0 || keys.some((k) => Number(k) === n)) return node;
    const lo = keys.filter((k) => Number(k) < n).pop();
    const hi = keys.find((k) => Number(k) > n);
    const w = lo !== undefined && hi !== undefined ? (n - Number(lo)) / (Number(hi) - Number(lo)) : 0;
    const v = blend(lo === undefined ? undefined : node[lo], hi === undefined ? undefined : node[hi], w);
    return v === undefined ? node : { ...node, [key]: v };
  });
}

/** The table without breakpoint `key` of axis `axis` (its slices go). */
export function removeBreakpoint(t: Grid, axis: number, key: string): Grid {
  return atLevel(t, axis, (node) => {
    const rest = { ...node };
    delete rest[key];
    return rest;
  });
}

/** Breakpoint `from` of axis `axis` written as `to` (a breakpoint moved). */
export function renameBreakpoint(t: Grid, axis: number, from: string, to: string): Grid {
  if (from === to) return t;
  return atLevel(t, axis, (node) => {
    const out: TableN = {};
    for (const [k, v] of Object.entries(node)) out[k === from ? to : k] = v;
    return out;
  });
}

/** The table without axis `axis`, keeping its slice at breakpoint `key`. */
export function dropAxis(t: Grid, axis: number, key: string): Grid {
  if (typeof t !== "object") return t;
  if (axis === 0) return t[key] ?? 0;
  const out: TableN = {};
  for (const [k, v] of Object.entries(t)) out[k] = dropAxis(v, axis - 1, key);
  return out;
}

/** The table with a new last axis of one breakpoint, "0": every value
 *  becomes that slice. */
export function addAxis(t: Grid): Grid {
  if (typeof t !== "object") return { "0": t };
  const out: TableN = {};
  for (const [k, v] of Object.entries(t)) out[k] = addAxis(v);
  return out;
}

/** A table of the same grid as `t`'s, every value `v`. */
export function filled(t: Grid, v: number): Grid {
  if (typeof t !== "object") return v;
  const out: TableN = {};
  for (const [k, x] of Object.entries(t)) out[k] = filled(x, v);
  return out;
}

/** A grid of one breakpoint, "0", on each of `depth` axes, holding `v`. */
export function unitGrid(depth: number, v = 0): Grid {
  let t: Grid = v;
  for (let d = 0; d < depth; d++) t = { "0": t };
  return t;
}

/** The value at every point of a full grid (last axis fastest), to read
 *  cells back. */
export function gridValues(t: Grid): number[] {
  if (typeof t !== "object") return [t];
  return Object.keys(t)
    .sort(byNumber)
    .flatMap((k) => gridValues(t[k]));
}
