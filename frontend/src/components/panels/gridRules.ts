// The rules every table editor shares: how a typed number or breakpoint is
// checked, how a stored key or value is shown, and the hint under a grid.
import type { GridIssue, GridRange } from "../SpreadsheetGrid";
import { showNumber, type UnitSlot } from "../../units";
import type { OutsidePolicy } from "../../types";

/** What a table does past an axis's data, in words. */
export const OUTSIDE_LABELS: Record<OutsidePolicy, string> = {
  error: "stop the run (Error)",
  clamp: "hold the edge value (Clamp)",
  linear: "extend the edge slope (Linear)",
};

/** A port's id from the name typed for it ("Motor Speed" → "motor_speed"). */
export function slugify(name: string): string {
  return name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "port";
}

export function sortedNumericKeys(obj: Record<string, unknown>): string[] {
  return Object.keys(obj).sort((a, b) => Number(a) - Number(b));
}

/** Next axis value after `keys`, extrapolating the last step (fallback if flat). */
export function nextAxisKey(keys: string[], fallback: number): number {
  const last = keys.length ? Number(keys[keys.length - 1]) : 0;
  const prev = keys.length > 1 ? Number(keys[keys.length - 2]) : last - fallback;
  const step = keys.length > 1 ? last - prev : fallback;
  return last + (step || fallback);
}

export const GRID_HINT =
  "Type or double-click to edit a cell · drag or Shift+click to select a range · Copy and Paste (Ctrl+C, Ctrl+V) work with Excel";

/** Whether a grid selection is one cell. */
export const oneCell = (r: GridRange | null) => !!r && r.r0 === r.r1 && r.c0 === r.c1;

/** Why `text` cannot be stored as a number, or null when it can. */
export function numberIssue(text: string): GridIssue | null {
  const t = text.trim();
  if (Number.isFinite(Number(t))) return null;
  const hint = /^-?\d+,\d+$/.test(t) ? " (use a point for decimals)" : "";
  return { level: "error", text: `'${t}' is not a number${hint}.` };
}

/** Check a typed breakpoint against the others on its axis (`keys`, in
 *  order; `index` is its own position): it must be a new number, and one out
 *  of order is flagged because the row or column will move. */
export function axisIssue(
  name: string,
  what: "row" | "column",
  text: string,
  keys: string[],
  index: number,
): GridIssue | null {
  const t = text.trim();
  if (t === "") return { level: "error", text: `Enter a ${name} value.` };
  const bad = numberIssue(t);
  if (bad) return bad;
  const n = Number(t);
  if (keys.some((k, i) => i !== index && Number(k) === n))
    return { level: "error", text: `${name} ${n} is already in the table.` };
  const prev = index > 0 ? Number(keys[index - 1]) : -Infinity;
  const next = index < keys.length - 1 ? Number(keys[index + 1]) : Infinity;
  if (n < prev || n > next)
    return { level: "warning", text: `${name} ${n} is out of order: the ${what} moves so ${name} keeps increasing.` };
  return null;
}

/** First problem among axis cells about to be pasted, or null. Empty cells
 *  are allowed only where the paste adds a new row/column (auto-numbered). */
export function pastedAxisIssue(name: string, cells: string[], existing: number): GridIssue | null {
  const seen = new Set<number>();
  for (let i = 0; i < cells.length; i++) {
    const t = cells[i].trim();
    if (t === "" && i >= existing) continue;
    if (t === "") return { level: "error", text: `a ${name} value is empty.` };
    const bad = numberIssue(t);
    if (bad) return bad;
    const n = Number(t);
    if (seen.has(n)) return { level: "error", text: `${name} ${n} would appear twice.` };
    seen.add(n);
  }
  return null;
}

/** A stored key or value as its cell shows it: in the library's unit as it
 *  is (a key as written), else converted (units.ts). */
export function cellText(stored: string | number, lib: string, unit: string): string {
  return unit === lib ? String(stored) : showNumber(Number(stored), lib, unit);
}

/** How a table's numbers are shown: the unit of each axis and of the values,
 *  and how to keep a choice (with the table's new value when the numbers
 *  were in another unit). The table itself is always in the library's units. */
export interface TableShown {
  axes: string[];
  value: string;
  set: (slot: UnitSlot, unit: string, table?: unknown) => void;
}

