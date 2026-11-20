// The units a parameter can be shown and typed in. The project file keeps
// every value in the library's unit; the unit chosen to work in is only how
// it is shown (ElementInstance.displayUnits). The conversions are the
// import's own table, backend/app/dataio/units.json (copied here as
// data/units.json by scripts/sync-data.mjs), so the two cannot disagree.
import table from "./data/units.json";
import type { ElementInstance } from "./types";

type Group = Record<string, [number, number]>;
const GROUP_OF = new Map<string, Group>();
for (const g of Object.values((table as unknown as { groups: Record<string, Group> }).groups))
  for (const u of Object.keys(g)) GROUP_OF.set(u, g);

/** The values of a parameter, or axis `i` of its table. */
export type UnitSlot = "value" | number;

/** The units a value stored in `lib` can be shown in, `lib` first; none when
 *  it has no other, or when `lib` is not the unit the library stores that
 *  quantity in (a PID gain in 1/s is not a rotational speed). */
export function alternatives(lib: string): string[] {
  const units = Object.keys(GROUP_OF.get(lib) ?? {});
  return units.length > 1 && units[0] === lib ? units : [];
}

/** `v` in `from` as a value in `to`, as the backend's units.convert does it. */
export function convert(v: number, from: string, to: string): number {
  if (from === to) return v;
  const g = GROUP_OF.get(from);
  if (!g?.[to]) throw new Error(`${from} cannot be converted to ${to}`);
  const [sa, oa] = g[from];
  const [sb, ob] = g[to];
  return v * (sa / sb) + (oa - ob) / sb;
}

/** A number typed in `unit` as the value stored in `lib`: converted once,
 *  without float noise (0.1 m/s is 0.36 km/h, not 0.36000000000000004).
 *  It keeps 12 significant digits, as the import's tidy() does, of the
 *  largest term, so an offset that cancels leaves no noise either (273.25 K
 *  is 0.1 °C, not 0.100000000000023). */
export function toStored(n: number, lib: string, unit: string): number {
  if (unit === lib || !Number.isFinite(n)) return n;
  const g = GROUP_OF.get(unit)!;
  const v = convert(n, unit, lib);
  const big = Math.max(Math.abs(v), Math.abs((n * g[unit][0]) / g[lib][0]));
  const decimals = big === 0 ? 0 : 11 - Math.floor(Math.log10(big));
  return decimals >= 0 && decimals <= 100 ? Number(v.toFixed(decimals)) : Number(v.toPrecision(12));
}

/** A stored value as text in `unit`: in the library's unit as it is, else
 *  the shortest text that reads back (toStored) as the same stored value. */
export function showNumber(v: number, lib: string, unit: string): string {
  if (unit === lib || !Number.isFinite(v)) return String(v);
  const x = convert(v, lib, unit);
  for (let p = 1; p <= 17; p++) {
    const t = String(Number(x.toPrecision(p)));
    if (toStored(Number(t), lib, unit) === v) return t;
  }
  // (a value with more digits than an edit keeps: a field left alone is
  // never written, and a table cell reads back through cellReader)
  return String(Number(x.toPrecision(12)));
}

/** A value for a sentence (a limit, a default, a typical range), to 4 digits. */
export function roundText(v: number, lib: string, unit: string): string {
  return unit === lib ? String(v) : String(Number(convert(v, lib, unit).toPrecision(4)));
}

/** "The number is in another unit": the number shown in `unit` stays as it
 *  is, read in `to`; returns the new stored value. */
export function reinterpret(v: number, lib: string, unit: string, to: string): number {
  return toStored(Number(showNumber(v, lib, unit)), lib, to);
}

/** Reads a table cell's text as a stored value: a text that shows one of
 *  `stored` gives that value exactly (an edit elsewhere in the table does not
 *  move the cells left alone), any other is converted once. */
export function cellReader(stored: number[], lib: string, unit: string): (text: string) => number {
  const known = new Map(stored.map((v) => [showNumber(v, lib, unit), v]));
  return (text) => {
    const t = String(text).trim();
    return known.get(t) ?? toStored(Number(t), lib, unit);
  };
}

/** The unit a parameter's values (or its table's axis) are shown in: the
 *  one chosen for this part when it is one of the alternatives, else the
 *  library's (a file without a choice, or with one this version does not know). */
export function shownUnit(element: ElementInstance, key: string, slot: UnitSlot, lib: string): string {
  const d = element.displayUnits?.[key];
  const u = slot === "value" ? d?.value : d?.axes?.[slot];
  return u && alternatives(lib).includes(u) ? u : lib;
}

/** A table (nested {key: …} to any depth, numbers at the bottom) with the
 *  breakpoints of axis `slot`, or else its values, mapped by `fn`. */
export function mapTable<T>(t: T, slot: UnitSlot, fn: (n: number) => number, depth = 0): T {
  if (typeof t === "number") return (slot === "value" ? fn(t) : t) as T;
  if (!t || typeof t !== "object") return t;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(t))
    out[slot === depth ? String(fn(Number(k))) : k] = mapTable(v, slot, fn, depth + 1);
  return out as T;
}

/** Every number of a table's axis `slot` (or its values), to read cells back. */
export function tableNumbers(t: unknown, slot: UnitSlot): number[] {
  const out: number[] = [];
  mapTable(t, slot, (n) => (out.push(n), n));
  return out;
}

const NUMBER = String.raw`-?\d[\d,]*(?:\.\d+)?`;

/** A help text with the numbers given in `lib` ("200–450 N·m") in `unit`. */
export function convertProse(text: string, lib: string, unit: string): string {
  if (unit === lib) return text;
  const u = lib.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
  const re = new RegExp(String.raw`(${NUMBER})(?:(\s*[–-]\s*)(${NUMBER}))?(\s*)${u}(?![\p{L}\d²³·/])`, "gu");
  const f = (s: string) => roundText(Number(s.replace(/,/g, "")), lib, unit);
  return text.replace(re, (_m, a: string, dash: string | undefined, b: string | undefined, sp: string) =>
    `${f(a)}${b ? `${dash}${f(b)}` : ""}${sp}${unit}`,
  );
}
