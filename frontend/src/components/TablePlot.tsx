// The plot under a table editor: it follows every edit, undo and paste, in
// the units the table is shown in. A table has any number of axes: the plot
// draws a slice of it, one axis along x and a curve per breakpoint of
// another, the rest held at one breakpoint each.
import { lazy, Suspense, useState } from "react";
import { ChartSpline } from "lucide-react";
import { useOwnerWindow } from "../ownerWindow";
import { convert, roundText, showNumber } from "../units";

// uPlot is loaded with the chart, not with the form: it needs a real browser
// (a test DOM has no canvas), and a form shown without a plot never loads it
const Chart = lazy(() => import("./TablePlotChart"));

/** One axis of a table as the editor shows it: its breakpoints (the stored
 *  keys, in order) and the unit they are stored in and shown in. */
export interface ShownAxis {
  name: string;
  lib: string;
  unit: string;
  keys: string[];
}

/** What a table plot draws, in the units shown. */
export interface TableSlice {
  x: { title: string; values: number[] };
  yTitle: string;
  /** the values' unit, for a read-out */
  yUnit: string;
  curves: { label: string; y: (number | null)[] }[];
  /** the point of the selected cell: its curve and its index along x */
  mark: { curve: number; index: number } | null;
}

// "-" is no unit: it is not written after a number or a name
const named = (unit: string) => (unit && unit !== "-" ? unit : "");
const title = (name: string, unit: string) => (named(unit) ? `${name} (${unit})` : name);
// a breakpoint as its cell shows it, or (a curve's name) to 4 digits
const keyText = (a: ShownAxis, i: number, show = showNumber) =>
  `${show(Number(a.keys[i]), a.lib, a.unit)}${named(a.unit) ? ` ${a.unit}` : ""}`;

/** The table's entry at one breakpoint of each axis (keys in axis order). */
const entryAt = (table: unknown, keys: string[]): unknown =>
  keys.reduce<unknown>((t, k) => (t && typeof t === "object" ? (t as Record<string, unknown>)[k] : undefined), table);

/** The default slice: along x the last axis (a grid's rows), a curve per
 *  breakpoint of the axis before it. */
export const defaultAlong = (axes: number) => Math.max(0, axes - 1);
const defaultBy = (axes: number, along: number) => (axes < 2 ? null : along === axes - 1 ? axes - 2 : axes - 1);

/**
 * A table (nested {key: …}, one level per axis, numbers at the bottom) as a
 * plot slice: axis `along` runs along x, each breakpoint of axis `by` is a
 * curve, every other axis stays at its breakpoint in `fixed` (index, default
 * the first). `mark` is the selected cell's index on each axis (null: the
 * selection is not one cell). A missing entry is a gap in its curve.
 */
export function tableSlice(
  table: unknown,
  axes: ShownAxis[],
  value: { name: string; lib: string; unit: string },
  along: number,
  opts: { by?: number | null; fixed?: number[]; mark?: number[] | null } = {},
): TableSlice {
  const by = opts.by === undefined ? defaultBy(axes.length, along) : opts.by;
  const xa = axes[along];
  const at = (i: number, curve: number) =>
    axes.map((a, j) => a.keys[j === along ? i : j === by ? curve : (opts.fixed?.[j] ?? 0)]);
  const curveCount = by === null ? 1 : axes[by].keys.length;
  const curves = Array.from({ length: curveCount }, (_, k) => ({
    label: by === null ? value.name : `${axes[by].name} ${keyText(axes[by], k, roundText)}`,
    y: xa.keys.map((_, i) => {
      const v = entryAt(table, at(i, k));
      return typeof v === "number" ? convert(v, value.lib, value.unit) : null;
    }),
  }));
  const m = opts.mark;
  const inSlice =
    m && m.length === axes.length && axes.every((_, j) => j === along || j === by || m[j] === (opts.fixed?.[j] ?? 0));
  return {
    x: { title: title(xa.name, xa.unit), values: xa.keys.map((k) => convert(Number(k), xa.lib, xa.unit)) },
    yTitle: title(value.name, value.unit),
    yUnit: value.unit,
    curves,
    mark: inSlice && m ? { curve: by === null ? 0 : m[by], index: m[along] } : null,
  };
}

/** The curve whose point at `idx` is nearest to `y` (none has a value: null). */
export function nearestCurve(slice: TableSlice, idx: number, y: number): number | null {
  let best: number | null = null;
  slice.curves.forEach((c, k) => {
    const v = c.y[idx];
    if (v != null && (best === null || Math.abs(v - y) < Math.abs(slice.curves[best].y[idx]! - y))) best = k;
  });
  return best;
}

/** A point of the plot (its curve and its index along x) as the index of
 *  its breakpoint on every axis: the slice's held ones for the rest. */
export function pointCell(axes: number, along: number, curve: number, index: number, fixed?: number[]): number[] {
  const by = defaultBy(axes, along);
  return Array.from({ length: axes }, (_, j) => (j === along ? index : j === by ? curve : (fixed?.[j] ?? 0)));
}

/** The selected cell in words: its breakpoints and its value, as the cells show them. */
export function markText(
  table: unknown,
  axes: ShownAxis[],
  value: { name: string; lib: string; unit: string },
  mark: number[] | null,
): string {
  if (!mark || mark.length !== axes.length || axes.some((a, j) => !a.keys[mark[j]])) return "";
  const v = entryAt(
    table,
    axes.map((a, j) => a.keys[mark[j]]),
  );
  const shown =
    typeof v === "number" ? `${showNumber(v, value.lib, value.unit)}${named(value.unit) ? ` ${value.unit}` : ""}` : "no value";
  return `${axes.map((a, j) => `${a.name} ${keyText(a, mark[j])}`).join(", ")}: ${value.name} ${shown}`;
}

/** The plot under a table, with a choice of the axis along x when there are
 *  several, and a line naming the selected cell's point. */
export function TablePlot({
  table,
  axes,
  value,
  mark,
  fixed,
  onPick,
}: {
  table: unknown;
  axes: ShownAxis[];
  value: { name: string; lib: string; unit: string };
  /** the selected cell's index on each axis, or null */
  mark: number[] | null;
  /** the breakpoint (index) each axis is held at when it is neither along x
   *  nor the curves': the slice the editor shows (default the first) */
  fixed?: number[];
  /** a point clicked on the plot: its index on each axis (its cell is selected) */
  onPick?: (cell: number[]) => void;
}) {
  const win = useOwnerWindow();
  const [open, setOpen] = useState(true);
  const [alongPick, setAlong] = useState<number | null>(null);
  const along = alongPick !== null && alongPick < axes.length ? alongPick : defaultAlong(axes.length);
  const slice = tableSlice(table, axes, value, along, { mark, fixed });
  const marked = markText(table, axes, value, mark);
  // a window without a layout (a test DOM) draws no chart
  const canDraw = "ResizeObserver" in win;
  return (
    <figure className="ss-table-plot m-0 flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          className="ss-toolbtn"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          title={open ? "Hide the plot of this table" : "Show the plot of this table"}
        >
          <ChartSpline size={12} aria-hidden /> {open ? "Hide plot" : "Show plot"}
        </button>
        {open && axes.length > 1 && (
          <label className="flex items-center gap-1 text-meta text-[color:var(--ss-text-dim)]">
            Along x
            <select
              className="ss-input py-0.5 text-meta"
              value={along}
              onChange={(e) => setAlong(Number(e.target.value))}
            >
              {axes.map((a, i) => (
                <option key={i} value={i}>
                  {title(a.name, a.unit)}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      {open && canDraw && (
        <div className="ss-table-plot-chart">
          <Suspense fallback={null}>
            <Chart
              slice={slice}
              onPick={onPick && ((curve, index) => onPick(pointCell(axes.length, along, curve, index, fixed)))}
            />
          </Suspense>
        </div>
      )}
      {open && (
        <figcaption data-plot-mark className="text-meta text-[color:var(--ss-text-dim)]">
          {marked
            ? `Marked: ${marked}`
            : `Select a cell to mark its point. Point at the plot to read its values${onPick ? ", click a point to select its cell" : ""}.`}
        </figcaption>
      )}
    </figure>
  );
}
