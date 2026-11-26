// The editor of a table of any number of axes as one full grid
// (docs/spec/tables.md): a library table of three or more axes, and the
// tables of a Map block's outputs, which share one grid. The grid shows two
// axes (rows and columns, the user's choice); every other axis is held at
// a breakpoint chosen above it. Breakpoints can be added and removed on any
// axis; a new one copies the nearest breakpoint's points, in every slice and
// every output, and the line under the controls says so.
import { useState, type ReactNode } from "react";
import { Columns3, Plus, Rows3, Trash2 } from "lucide-react";
import { SpreadsheetGrid, type GridCell, type GridIssue, type GridLine, type GridRange } from "../SpreadsheetGrid";
import { TableUnits, type UnitColumn } from "../UnitChooser";
import { TablePlot } from "../TablePlot";
import { alternatives, cellReader, mapTable, reinterpret, toStored, type UnitSlot } from "../../units";
import {
  addBreakpoint,
  getAt,
  gridKeys,
  gridValues,
  insertBreakpoint,
  nearest,
  removeBreakpoint,
  renameBreakpoint,
  setAt,
  type Grid,
} from "../../tableN";
import { continueKeys, newBreakpoint } from "../gridTools";
import { GRID_HINT, axisIssue, cellText, nextAxisKey, numberIssue, oneCell, pastedAxisIssue } from "./gridRules";

/** An axis or a table's values as the editor shows them: the unit the numbers
 *  are stored in (`lib`) and the one they are shown in. */
export interface ShownQuantity {
  name: string;
  lib: string;
  unit: string;
  /** a unit the app does not know (a Map port's unit text): shown, never
   *  converted, and no unit chooser (lib and unit are that text) */
  fixed?: boolean;
}

/** One table of the grid: a library table's own ("value"), or a Map
 *  block's output (its port id). */
export interface GridOutput extends ShownQuantity {
  key: string;
}

/** What a unit choice stores: the unit of an axis (by index) or of an
 *  output's values, and, when the numbers were in another unit, the tables
 *  read in it. */
export type SetGridUnit = (slot: UnitSlot, output: string, unit: string, tables?: Record<string, Grid>) => void;

const title = (q: ShownQuantity) => (q.unit && q.unit !== "-" ? `${q.name} (${q.unit})` : q.name);

export function TableNEditor({
  axes,
  outputs,
  tables,
  onChange,
  setUnit,
  tools,
  onImportText,
}: {
  axes: ShownQuantity[];
  outputs: GridOutput[];
  tables: Record<string, Grid>;
  onChange: (tables: Record<string, Grid>) => void;
  setUnit: SetGridUnit;
  tools?: ReactNode;
  /** pasted text the grid cannot take as it is goes to the import's preview */
  onImportText?: (text: string, why: string) => void;
}) {
  const depth = axes.length;
  const [outPick, setOut] = useState<string | null>(null);
  const out = outputs.find((o) => o.key === outPick) ?? outputs[0];
  const [shown, setShown] = useState<[number, number]>([Math.max(0, depth - 1), Math.max(0, depth - 2)]);
  const rowAxis = shown[0] < depth ? shown[0] : Math.max(0, depth - 1);
  const colAxis = depth < 2 ? null : shown[1] < depth && shown[1] !== rowAxis ? shown[1] : rowAxis === 0 ? 1 : 0;
  const [held, setHeld] = useState<Record<number, string>>({});
  const [selRange, setSelRange] = useState<GridRange | null>(null);
  const [unitOpen, setUnitOpen] = useState<UnitSlot | null>(null);
  const [adding, setAdding] = useState<{ axis: number; text: string } | null>(null);
  const [note, setNote] = useState("");
  const [focusCell, setFocusCell] = useState<{ r: number; c: number } | null>(null);

  const keys = gridKeys(tables[outputs[0].key] ?? {}, depth);
  const hidden = axes.map((_, j) => j).filter((j) => j !== rowAxis && j !== colAxis);
  const heldKey = (j: number) => (keys[j].includes(held[j]) ? held[j] : keys[j][0]);
  const show = (j: number, k: string) => cellText(k, axes[j].lib, axes[j].unit);
  const keyReader = (j: number) => cellReader(keys[j].map(Number), axes[j].lib, axes[j].unit);
  const table = tables[out.key] ?? {};
  const readV = cellReader(gridValues(table), out.lib, out.unit);
  const rowKeys = keys[rowAxis];
  const colKeys = colAxis === null ? [""] : keys[colAxis];

  /** The breakpoint of each axis at row `i` and column `j` of the grid. */
  const pathAt = (i: number, j: number) =>
    axes.map((_, a) => (a === rowAxis ? rowKeys[i] : a === colAxis ? colKeys[j] : heldKey(a)));
  const valueAt = (i: number, j: number) => {
    const v = getAt(table, pathAt(i, j));
    return typeof v === "number" ? cellText(v, out.lib, out.unit) : "";
  };
  const every = (fn: (t: Grid) => Grid) =>
    Object.fromEntries(outputs.map((o) => [o.key, fn(tables[o.key] ?? {})]));
  // "Temp now has a breakpoint at 8 °C; its values are copied from the one at 10 °C"
  const copiedNote = (j: number, key: string, from: string) => {
    const at = (k: string) => `${show(j, k)}${axes[j].unit !== "-" ? ` ${axes[j].unit}` : ""}`;
    setNote(
      `${axes[j].name} now has a breakpoint at ${at(key)}; its values are copied from the one at ${at(from)}` +
        `${outputs.length > 1 ? ", in every output" : ""}.`,
    );
  };

  // the grid: one axis down the rows, another along the columns (one axis:
  // its breakpoints and a column of values)
  const matrix: GridCell[][] =
    colAxis === null
      ? [
          [
            { text: title(axes[0]), kind: "colHeader", readOnly: true },
            { text: title(out), kind: "colHeader", readOnly: true },
          ],
          ...rowKeys.map((k, i): GridCell[] => [
            { text: show(rowAxis, k), kind: "body", axis: true },
            { text: valueAt(i, 0), kind: "body" },
          ]),
        ]
      : [
          [
            { text: title(out), kind: "corner", readOnly: true },
            ...colKeys.map((k): GridCell => ({ text: show(colAxis, k), kind: "colHeader" })),
          ],
          ...rowKeys.map((k, i): GridCell[] => [
            { text: show(rowAxis, k), kind: "rowHeader" },
            ...colKeys.map((_, j): GridCell => ({ text: valueAt(i, j), kind: "body" })),
          ]),
        ];
  const isRowKey = (r: number, c: number) => r >= 1 && c === 0;
  const isColKey = (r: number, c: number) => colAxis !== null && r === 0 && c >= 1;

  const validate = (r: number, c: number, text: string): GridIssue | null => {
    if (isRowKey(r, c)) return axisIssue(axes[rowAxis].name, "row", text, rowKeys.map((k) => show(rowAxis, k)), r - 1);
    if (isColKey(r, c))
      return axisIssue(axes[colAxis!].name, "column", text, colKeys.map((k) => show(colAxis!, k)), c - 1);
    if (r === 0) return null;
    if (text.trim() === "") return { level: "error", text: `Enter a ${out.name} value (Delete sets 0).` };
    return numberIssue(text);
  };

  /** The tables with the shown grid's text `g` written back: breakpoints
   *  renamed or added (a new one copies its nearest), then every value. */
  const writeBack = (g: string[][]): Record<string, Grid> => {
    let next = { ...tables };
    const each = (fn: (t: Grid) => Grid) => (next = Object.fromEntries(outputs.map((o) => [o.key, fn(next[o.key] ?? {})])));
    const place = (axis: number, old: string[], texts: string[]): string[] => {
      const read = keyReader(axis);
      // (a breakpoint a paste past the end did not give continues the last step)
      const want = continueKeys(texts).map((t) => String(read(t.trim())));
      // (two steps, so breakpoints that trade places never meet)
      old.forEach((k, i) => i < want.length && k !== want[i] && each((t) => renameBreakpoint(t, axis, k, `\u0000${i}`)));
      old.forEach((k, i) => i < want.length && k !== want[i] && each((t) => renameBreakpoint(t, axis, `\u0000${i}`, want[i])));
      want.slice(old.length).forEach((k) => {
        const from = nearest(gridKeys(next[outputs[0].key], depth)[axis], Number(k));
        each((t) => addBreakpoint(t, axis, k));
        copiedNote(axis, k, from);
      });
      return want;
    };
    const rows = place(rowAxis, rowKeys, g.slice(1).map((row) => row[0] ?? ""));
    const cols = colAxis === null ? [""] : place(colAxis, colKeys, g[0].slice(1));
    let t = next[out.key];
    rows.forEach((rk, i) =>
      cols.forEach((ck, j) => {
        const text = (g[i + 1]?.[j + 1] ?? "").trim();
        const path = axes.map((_, a) => (a === rowAxis ? rk : a === colAxis ? ck : heldKey(a)));
        if (text !== "" && Number.isFinite(Number(text))) t = setAt(t, path, readV(text));
      }),
    );
    return { ...next, [out.key]: t };
  };
  const shownText = () => matrix.map((row) => row.map((cell) => cell.text));

  const commit = (r: number, c: number, text: string) => {
    if (r === 0 && !isColKey(r, c)) return;
    const g = shownText();
    g[r][c] = text;
    setNote("");
    onChange(writeBack(g));
  };

  const pasteBlock = (r: number, c: number, block: string[][]): string | void => {
    const g = shownText();
    const wide = c + Math.max(...block.map((b) => b.length));
    if (colAxis === null && wide > 2)
      return `Paste not applied: the clipboard has ${wide - c} columns, and from here the table has ${2 - c}.`;
    const width = colAxis === null ? 2 : Math.max(g[0].length, wide);
    while (g.length < r + block.length) g.push([""]);
    for (const row of g) while (row.length < width) row.push("");
    block.forEach((brow, bi) =>
      brow.forEach((val, bj) => {
        const [tr, tc] = [r + bi, c + bj];
        if (tc >= width || (tr === 0 && !isColKey(tr, tc))) return;
        g[tr][tc] = val;
      }),
    );
    const problem =
      pastedAxisIssue(axes[rowAxis].name, g.slice(1).map((row) => row[0]), rowKeys.length) ??
      (colAxis === null ? null : pastedAxisIssue(axes[colAxis].name, g[0].slice(1), colKeys.length)) ??
      g
        .slice(1)
        .flatMap((row, i) => row.slice(1).map((t, j) => ({ t, fresh: i >= rowKeys.length || j >= colKeys.length })))
        .map(({ t, fresh }) => (t.trim() === "" ? (fresh ? null : { level: "error", text: `a ${out.name} value is empty.` }) : numberIssue(t)))
        .find(Boolean);
    if (problem) return `Paste not applied: ${problem.text}`;
    setNote("");
    onChange(writeBack(g));
  };

  const clearRange = (cells: { r: number; c: number }[]) => {
    const g = shownText();
    for (const { r, c } of cells) if (r >= 1 && c >= 1) g[r][c] = "0";
    onChange(writeBack(g));
  };

  const addOn = (axis: number, key: string) => {
    const from = nearest(keys[axis], Number(key));
    onChange(every((t) => addBreakpoint(t, axis, key)));
    copiedNote(axis, key, from);
  };
  const removeOn = (axis: number, drop: string[]) => {
    if (drop.length === 0 || drop.length >= keys[axis].length) return;
    setNote("");
    onChange(every((t) => drop.reduce((acc, k) => removeBreakpoint(acc, axis, k), t)));
  };
  const between = (lo: number, hi: number, list: string[]) => list.filter((_, i) => i + 1 >= Math.max(1, lo) && i + 1 <= hi);

  // a right-click on a row or column header: a breakpoint inserted (midway,
  // its points interpolated, so the table means the same) or deleted, along
  // the whole table: every slice of the other axes, every output
  const lineAxis = (line: GridLine) => (line === "row" ? rowAxis : colAxis!);
  const insertAt = (line: GridLine, at: number, where: "before" | "after") => {
    const axis = lineAxis(line);
    const key = newBreakpoint(keys[axis], at - 1, where);
    onChange(every((t) => insertBreakpoint(t, axis, key)));
    const unit = axes[axis].unit && axes[axis].unit !== "-" ? ` ${axes[axis].unit}` : "";
    const ends = at - 1 === (where === "before" ? 0 : keys[axis].length - 1);
    setNote(
      `Inserted ${axes[axis].name} ${show(axis, key)}${unit}: its points are ${ends ? "a copy of the edge's" : "interpolated between its neighbours"}` +
        `${depth > 2 ? ", in every slice" : ""}${outputs.length > 1 ? ", in every output" : ""}.`,
    );
  };
  const deleteAt = (line: GridLine, at: number[]) => {
    const axis = lineAxis(line);
    removeOn(axis, at.map((i) => keys[axis][i - 1]).filter((k) => k !== undefined));
  };
  // a point clicked on the plot: its slice is shown and its cell selected
  const pickPoint = (idx: number[]) => {
    if (hidden.some((j) => keys[j][idx[j]] !== heldKey(j)))
      setHeld({ ...held, ...Object.fromEntries(hidden.map((j) => [j, keys[j][idx[j]]])) });
    setFocusCell({ r: idx[rowAxis] + 1, c: colAxis === null ? 1 : idx[colAxis] + 1 });
  };

  // a held axis's breakpoint typed in its shown unit
  const addTyped = () => {
    if (!adding) return;
    const t = adding.text.trim();
    const bad =
      numberIssue(t) ??
      (t === "" ? { text: `Enter a ${axes[adding.axis].name} value.` } : null) ??
      (keys[adding.axis].some((k) => show(adding.axis, k) === t || Number(k) === toStored(Number(t), axes[adding.axis].lib, axes[adding.axis].unit))
        ? { text: `${axes[adding.axis].name} ${t} is already in the table.` }
        : null);
    if (bad) return setNote(bad.text);
    const key = String(toStored(Number(t), axes[adding.axis].lib, axes[adding.axis].unit));
    addOn(adding.axis, key);
    setHeld({ ...held, [adding.axis]: key });
    setAdding(null);
  };

  const columns: UnitColumn[] = [
    ...axes.map((a, j): UnitColumn => ({
      slot: j,
      label: j === rowAxis ? `Rows: ${a.name}` : j === colAxis ? `Columns: ${a.name}` : a.name,
      what: `the ${a.name} axis`,
      lib: a.fixed ? "" : a.lib,
      unit: a.unit,
    })),
    {
      slot: "value",
      label: `Values: ${out.name}`,
      what: `the ${out.name} values`,
      lib: out.fixed ? "" : out.lib,
      unit: out.unit,
    },
  ];
  const storeUnit = (slot: UnitSlot, unit: string, again?: unknown) => {
    if (again === undefined) return setUnit(slot, out.key, unit);
    const col = columns.find((c) => c.slot === slot)!;
    const read = (t: Grid) => mapTable(t, slot, (n) => reinterpret(n, col.lib, col.unit, unit));
    setUnit(slot, out.key, unit, slot === "value" ? { ...tables, [out.key]: read(table) } : every(read));
  };
  const openHeader = (r: number, c: number) => {
    const slot: UnitSlot | null = colAxis === null ? (r === 0 ? (c === 0 ? 0 : "value") : null) : r === 0 && c === 0 ? "value" : null;
    const col = columns.find((x) => x.slot === slot);
    if (slot === null || !col || alternatives(col.lib).length === 0) return false;
    setUnitOpen(slot);
    return true;
  };

  const one = oneCell(selRange) && selRange!.r0 >= 1 && selRange!.c0 >= 1;
  const mark = one
    ? axes.map((_, a) =>
        a === rowAxis ? selRange!.r0 - 1 : a === colAxis ? selRange!.c0 - 1 : keys[a].indexOf(heldKey(a)),
      )
    : null;
  const pick = "ss-input py-0.5 text-meta";
  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-meta text-[color:var(--ss-text-dim)]">
        {outputs.length > 1 && (
          <label className="flex items-center gap-1">
            Output
            <select className={pick} value={out.key} onChange={(e) => setOut(e.target.value)}>
              {outputs.map((o) => (
                <option key={o.key} value={o.key}>
                  {o.name}
                </option>
              ))}
            </select>
          </label>
        )}
        {depth > 1 && (
          <>
            <label className="flex items-center gap-1">
              Rows
              <select
                className={pick}
                value={rowAxis}
                onChange={(e) => {
                  const r = Number(e.target.value);
                  setShown([r, r === colAxis ? rowAxis : colAxis!]);
                }}
              >
                {axes.map((a, j) => (
                  <option key={j} value={j}>
                    {a.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-1">
              Columns
              <select
                className={pick}
                value={colAxis ?? 0}
                onChange={(e) => {
                  const c = Number(e.target.value);
                  setShown([c === rowAxis ? colAxis! : rowAxis, c]);
                }}
              >
                {axes.map((a, j) => (
                  <option key={j} value={j}>
                    {a.name}
                  </option>
                ))}
              </select>
            </label>
          </>
        )}
      </div>
      {hidden.length > 0 && (
        <div className="ss-table-held flex flex-wrap items-center gap-x-3 gap-y-1 text-meta" aria-label="Held axes">
          {hidden.map((j) => (
            <span key={j} className="flex items-center gap-1">
              <label className="flex items-center gap-1">
                {title(axes[j])}
                <select className={pick} value={heldKey(j)} onChange={(e) => setHeld({ ...held, [j]: e.target.value })}>
                  {keys[j].map((k) => (
                    <option key={k} value={k}>
                      {show(j, k)}
                    </option>
                  ))}
                </select>
              </label>
              {adding?.axis === j ? (
                <input
                  className="ss-input w-[70px] py-0.5 text-meta"
                  aria-label={`New ${axes[j].name} breakpoint`}
                  // what to type, on screen too (#31)
                  placeholder={axes[j].unit !== "-" ? `New ${axes[j].unit}` : "New value"}
                  title={`A new ${axes[j].name} breakpoint: type it and press Enter (Esc: cancel)`}
                  autoFocus
                  value={adding.text}
                  onChange={(e) => setAdding({ axis: j, text: e.target.value })}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") addTyped();
                    if (e.key === "Escape") setAdding(null);
                  }}
                  onBlur={() => setAdding(null)}
                />
              ) : (
                <button
                  type="button"
                  className="ss-toolbtn"
                  title={`Add a ${axes[j].name} breakpoint: its points copy the nearest one's`}
                  onClick={() => setAdding({ axis: j, text: "" })}
                >
                  <Plus size={12} aria-hidden /> Add
                </button>
              )}
              <button
                type="button"
                className="ss-toolbtn"
                disabled={keys[j].length <= 1}
                title={`Remove ${axes[j].name} ${show(j, heldKey(j))} and its points`}
                onClick={() => removeOn(j, [heldKey(j)])}
              >
                <Trash2 size={12} aria-hidden /> Remove
              </button>
            </span>
          ))}
        </div>
      )}
      {note && (
        <p role="status" className="text-meta text-[color:var(--ss-text-dim)]">
          {note}
        </p>
      )}
      <SpreadsheetGrid
        matrix={matrix}
        onCommit={commit}
        onPasteBlock={pasteBlock}
        onClearRange={clearRange}
        onSelectionChange={setSelRange}
        validate={validate}
        onHeaderOpen={openHeader}
        onImportText={onImportText}
        shade={depth >= 2 ? "map" : "curve"}
        onInsert={insertAt}
        onDelete={deleteAt}
        focusCell={focusCell}
        caption={<TableUnits columns={columns} table={table} open={unitOpen} setOpen={setUnitOpen} set={storeUnit} />}
        tools={
          <>
            <span className="ss-table-tools-sep" aria-hidden />
            {colAxis !== null && (
              <button
                type="button"
                className="ss-toolbtn"
                onClick={() => addOn(colAxis, String(nextAxisKey(colKeys, 1)))}
                title={`Add a ${axes[colAxis].name} column at the end: its points copy the last one's`}
              >
                <Columns3 size={12} aria-hidden /> Add {axes[colAxis].name}
              </button>
            )}
            <button
              type="button"
              className="ss-toolbtn"
              onClick={() => addOn(rowAxis, String(nextAxisKey(rowKeys, 1)))}
              title={`Add a ${axes[rowAxis].name} row at the end: its points copy the last one's`}
            >
              <Rows3 size={12} aria-hidden /> Add {axes[rowAxis].name}
            </button>
            {colAxis !== null && (
              <button
                type="button"
                className="ss-toolbtn"
                disabled={colKeys.length <= 1}
                onClick={() => selRange && removeOn(colAxis, between(selRange.c0, selRange.c1, colKeys))}
                title="Delete the selected columns, in every slice"
              >
                <Trash2 size={12} aria-hidden /> Columns
              </button>
            )}
            <button
              type="button"
              className="ss-toolbtn"
              disabled={rowKeys.length <= 1}
              onClick={() => selRange && removeOn(rowAxis, between(selRange.r0, selRange.r1, rowKeys))}
              title="Delete the selected rows, in every slice"
            >
              <Trash2 size={12} aria-hidden /> Rows
            </button>
            {tools}
          </>
        }
      />
      <p className="text-meta leading-tight text-[color:var(--ss-text-dim)]">
        {GRID_HINT}
        {hidden.length > 0 && " · Copy and Paste work on the slice shown"}
      </p>
      <TablePlot
        table={table}
        axes={axes.map((a, j) => ({ ...a, keys: keys[j] }))}
        value={out}
        mark={mark}
        fixed={axes.map((_, j) => Math.max(0, keys[j].indexOf(heldKey(j))))}
        onPick={pickPoint}
      />
    </div>
  );
}
