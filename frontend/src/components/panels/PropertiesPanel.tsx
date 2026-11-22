import { Fragment, useEffect, useId, useMemo, useRef, useState } from "react";
import { Box, Columns3, CornerDownRight, Pencil, Plus, Radio, Rows3, Table2, Trash2 } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { componentHelpPage, openHelp, openHelpWindow } from "../../help";
import { useOwnerWindow } from "../../ownerWindow";
import { limitsText, paramName, rangeProblem } from "../../paramRules";
import { LABEL_RRC, describeTyre, parseTyreCode, tyreValues } from "../../tyre";
import { SpreadsheetGrid, type GridCell, type GridIssue, type GridLine, type GridRange } from "../SpreadsheetGrid";
import { continueKeys, newBreakpoint } from "../gridTools";
import { insertBreakpoint, removeBreakpoint } from "../../tableN";
import { KIND_COLOR } from "../canvas/ElementNode";
import { CyclePreview, CycleSelect } from "./CyclePicker";
import { SourceBadge, ValueSources } from "./ValueSources";
import { ImportFromFileButton, type ImportTarget } from "../ImportTableDialog";
import { useTableImport } from "../ImportDataDialog";
import { TableUnits, UnitButton, UnitChooser, type UnitColumn } from "../UnitChooser";
import { TablePlot } from "../TablePlot";
import {
  alternatives,
  cellReader,
  convertProse,
  reinterpret,
  shownUnit,
  showNumber,
  tableNumbers,
  toStored,
  type UnitSlot,
} from "../../units";
import { FmuPanel } from "./FmuPanel";
import { MAP_DEF_ID, MapEditor } from "./MapEditor";
import { TableNEditor } from "./TableNEditor";
import {
  GRID_HINT,
  OUTSIDE_LABELS,
  axisIssue,
  cellText,
  nextAxisKey,
  numberIssue,
  oneCell,
  pastedAxisIssue,
  slugify,
  sortedNumericKeys,
  type TableShown,
} from "./gridRules";
import { FMU_DEF_ID, FMU_FILE_KEYS } from "../../fmu";
import type { DataTargetRef } from "../../api";
import type {
  AxisDef,
  ComponentDef,
  ElementInstance,
  OutsidePolicy,
  ParameterDef,
  ParamValue,
  PortDef,
  ScalarValue,
  Table1D,
  Table2D,
  TableN,
} from "../../types";
import { optionLabel } from "../../notYet";
import { showCaseValue } from "../../paramWindows";
import { PARAM_SECTIONS_KEY } from "../../storageKeys";

/** A library table of three or more axes: its grid, with the import. */
function LibraryTableN({ target, ...props }: Parameters<typeof TableNEditor>[0] & { target: DataTargetRef }) {
  const importer = useTableImport(target, (r) => props.onChange(r.tables));
  return (
    <>
      <TableNEditor {...props} tools={importer.tools} onImportText={importer.onImportText} />
      {importer.dialog}
    </>
  );
}

/** Excel-style editable grid for a 1D lookup table {x: y}. */
function Table1DEditor({
  value,
  axis,
  valueLabel,
  valueUnit,
  units,
  onChange,
  importTarget,
  onImport,
}: {
  value: Table1D;
  axis: AxisDef;
  valueLabel: string;
  valueUnit: string;
  units: TableShown;
  onChange: (v: Table1D) => void;
  /** offers "Import from file…" for this parameter (STD-10) */
  importTarget?: ImportTarget;
  onImport?: (v: Table1D | Table2D | string) => void;
}) {
  const [selRange, setSelRange] = useState<GridRange | null>(null);
  const [unitOpen, setUnitOpen] = useState<UnitSlot | null>(null);
  const [focusCell, setFocusCell] = useState<{ r: number; c: number } | null>(null);
  // a table's import (any layout, from a file or pasted); a profile keeps its own
  const importer = useTableImport(
    importTarget && importTarget.paramKey !== "profile" ? importTarget : null,
    (r) => onChange(r.tables.value as Table1D),
  );
  const keys = sortedNumericKeys(value);
  // what the cells show, and how their text reads back (units.ts)
  const xUnit = units.axes[0] ?? axis.unit;
  const yUnit = units.value;
  const pairs: [string, string][] = keys.map((k) => [
    cellText(k, axis.unit, xUnit),
    cellText(value[k], valueUnit, yUnit),
  ]);
  const shownKeys = pairs.map((p) => p[0]);
  const readX = cellReader(keys.map(Number), axis.unit, xUnit);
  const readY = cellReader(Object.values(value), valueUnit, yUnit);
  const columns: UnitColumn[] = [
    { slot: 0, label: axis.name, what: `the ${axis.name} axis`, lib: axis.unit, unit: xUnit },
    { slot: "value", label: valueLabel, what: `the ${valueLabel} values`, lib: valueUnit, unit: yUnit },
  ];

  const rebuild = (rows: [string, string][]): Table1D => {
    const obj: Table1D = {};
    for (const [x, y] of rows) {
      const xs = String(x).trim();
      const nx = Number(xs);
      if (xs === "" || !Number.isFinite(nx)) continue;
      const ny = Number(String(y).trim());
      obj[String(readX(xs))] = Number.isFinite(ny) ? readY(y) : 0;
    }
    return obj;
  };

  const matrix: GridCell[][] = [
    [
      { text: `${axis.name} (${xUnit})`, kind: "colHeader", readOnly: true },
      { text: `${valueLabel} (${yUnit})`, kind: "colHeader", readOnly: true },
    ],
    ...pairs.map(
      ([x, y]): GridCell[] => [
        { text: x, kind: "body", axis: true },
        { text: y, kind: "body" },
      ],
    ),
  ];
  // a double-click (or Enter, F2) on a header with a unit opens its chooser
  const openHeader = (r: number, c: number) => {
    const col = columns[c];
    if (r !== 0 || !col || alternatives(col.lib).length === 0) return false;
    setUnitOpen(col.slot);
    return true;
  };

  const validate = (r: number, c: number, text: string): GridIssue | null => {
    if (r === 0) return null;
    if (c === 0) return axisIssue(axis.name, "row", text, shownKeys, r - 1);
    if (text.trim() === "") return { level: "error", text: `Enter a ${valueLabel} value (Delete sets 0).` };
    return numberIssue(text);
  };

  const commit = (r: number, c: number, text: string) => {
    if (r === 0) return;
    const rows = pairs.map((p) => [...p] as [string, string]);
    if (!rows[r - 1]) return;
    rows[r - 1][c] = text;
    onChange(rebuild(rows));
  };

  const pasteBlock = (r: number, c: number, block: string[][]): string | void => {
    const rows = pairs.map((p) => [...p] as [string, string]);
    const startRow = r <= 0 ? 0 : r - 1;
    const startCol = Math.min(1, Math.max(0, c));
    const width = Math.max(...block.map((b) => b.length));
    if (startCol + width > 2)
      return `Paste not applied: the clipboard has ${width} columns, and from here the table has ${2 - startCol}.`;
    block.forEach((brow, bi) => {
      brow.forEach((val, bj) => {
        const tc = startCol + bj;
        if (tc > 1) return;
        const tr = startRow + bi;
        while (rows.length <= tr) rows.push(["", "0"]);
        rows[tr][tc] = val;
      });
    });
    // a breakpoint a paste past the end did not give continues the last step
    const xs = continueKeys(rows.map((row) => row[0]));
    rows.forEach((row, i) => i >= pairs.length && (row[0] = xs[i]));
    const problem =
      pastedAxisIssue(axis.name, rows.map((row) => row[0]), rows.length) ??
      rows
        .map((row): GridIssue | null =>
          row[1].trim() === "" ? { level: "error", text: `a ${valueLabel} value is empty.` } : numberIssue(row[1]),
        )
        .find(Boolean);
    if (problem) return `Paste not applied: ${problem.text}`;
    onChange(rebuild(rows));
  };

  const clearRange = (cells: { r: number; c: number }[]) => {
    const rows = pairs.map((p) => [...p] as [string, string]);
    for (const { r, c } of cells) {
      if (r >= 1 && c === 1 && rows[r - 1]) rows[r - 1][1] = "0";
    }
    onChange(rebuild(rows));
  };

  const addRow = () =>
    onChange({
      ...value,
      [String(nextAxisKey(keys, 500))]: keys.length ? value[keys[keys.length - 1]] : 0,
    });

  const deleteRows = () => {
    if (!selRange) return;
    const r0 = Math.max(1, selRange.r0);
    const rows = pairs.filter((_, i) => !(i + 1 >= r0 && i + 1 <= selRange.r1));
    if (rows.length === 0) return;
    onChange(rebuild(rows));
  };
  // a right-click on a breakpoint: a row inserted midway (its value
  // interpolated, so the curve is the same) or past an end, or rows deleted
  const insertAt = (_line: GridLine, at: number, where: "before" | "after") =>
    onChange(insertBreakpoint(value, 0, newBreakpoint(keys, at - 1, where)) as Table1D);
  const deleteAt = (_line: GridLine, at: number[]) => {
    const rows = pairs.filter((_, i) => !at.includes(i + 1));
    if (rows.length > 0) onChange(rebuild(rows));
  };

  return (
    <div className="flex flex-col gap-1">
      <SpreadsheetGrid
        matrix={matrix}
        onCommit={commit}
        onPasteBlock={pasteBlock}
        onClearRange={clearRange}
        onSelectionChange={setSelRange}
        validate={validate}
        onHeaderOpen={openHeader}
        onImportText={importer.onImportText}
        shade="curve"
        onInsert={insertAt}
        onDelete={deleteAt}
        focusCell={focusCell}
        caption={
          columns.some((c) => alternatives(c.lib).length > 0) && (
            <TableUnits columns={columns} table={value} open={unitOpen} setOpen={setUnitOpen} set={units.set} />
          )
        }
        tools={
          <>
            <span className="ss-table-tools-sep" aria-hidden />
            <button type="button" className="ss-toolbtn" onClick={addRow} title={`Add a ${axis.name} row at the end`}>
              <Plus size={12} aria-hidden /> Add row
            </button>
            <button
              type="button"
              className="ss-toolbtn"
              disabled={pairs.length <= 1}
              onClick={deleteRows}
              title="Delete the selected rows"
            >
              <Trash2 size={12} aria-hidden /> Delete rows
            </button>
            {importer.tools}
            {importTarget?.paramKey === "profile" && (
              <ImportFromFileButton target={importTarget} onApply={onImport ?? ((v) => onChange(v as Table1D))} />
            )}
          </>
        }
      />
      {importer.dialog}
      <p className="text-meta leading-tight text-[color:var(--ss-text-dim)]">{GRID_HINT}</p>
      <TablePlot
        table={value}
        axes={[{ name: axis.name, lib: axis.unit, unit: xUnit, keys }]}
        value={{ name: valueLabel, lib: valueUnit, unit: yUnit }}
        mark={oneCell(selRange) && selRange!.r0 >= 1 ? [selRange!.r0 - 1] : null}
        onPick={(idx) => setFocusCell({ r: idx[0] + 1, c: 1 })}
      />
    </div>
  );
}

/**
 * Excel-style editable 2D map {outer: {inner: y}} shown as a full matrix:
 * columns are the outer axis (e.g. voltage sheets), rows the inner axis
 * (e.g. speed), body cells the dependent value. Paste a whole block from Excel.
 */
function Table2DEditor({
  value,
  axes,
  valueLabel,
  valueUnit,
  units,
  onChange,
  importTarget,
}: {
  value: Table2D;
  axes: [AxisDef, AxisDef];
  valueLabel: string;
  valueUnit: string;
  units: TableShown;
  onChange: (v: Table2D) => void;
  importTarget?: ImportTarget;
}) {
  const [selRange, setSelRange] = useState<GridRange | null>(null);
  const [unitOpen, setUnitOpen] = useState<UnitSlot | null>(null);
  const [focusCell, setFocusCell] = useState<{ r: number; c: number } | null>(null);
  const importer = useTableImport(importTarget ?? null, (r) => onChange(r.tables.value as Table2D));
  const outerKeys = sortedNumericKeys(value);
  const innerSet = new Set<string>();
  for (const ok of outerKeys) for (const ik of Object.keys(value[ok] ?? {})) innerSet.add(ik);
  const innerKeys = [...innerSet].sort((a, b) => Number(a) - Number(b));
  // what the cells show, and how their text reads back (units.ts)
  const [cUnit, rUnit] = [units.axes[0] ?? axes[0].unit, units.axes[1] ?? axes[1].unit];
  const showC = (k: string) => cellText(k, axes[0].unit, cUnit);
  const showR = (k: string) => cellText(k, axes[1].unit, rUnit);
  const showV = (v: number | undefined) => (v === undefined ? "" : cellText(v, valueUnit, units.value));
  const readC = cellReader(outerKeys.map(Number), axes[0].unit, cUnit);
  const readR = cellReader(innerKeys.map(Number), axes[1].unit, rUnit);
  const readV = cellReader(tableNumbers(value, "value"), valueUnit, units.value);
  const columns: UnitColumn[] = [
    { slot: 0, label: `Columns: ${axes[0].name}`, what: `the ${axes[0].name} axis`, lib: axes[0].unit, unit: cUnit },
    { slot: 1, label: `Rows: ${axes[1].name}`, what: `the ${axes[1].name} axis`, lib: axes[1].unit, unit: rUnit },
    {
      slot: "value",
      label: `Values: ${valueLabel}`,
      what: `the ${valueLabel} values`,
      lib: valueUnit,
      unit: units.value,
    },
  ];

  /** Current grid as strings: row 0 = ["", ...outer], each row = [inner, ...values]. */
  const gridText = (): string[][] => {
    const g: string[][] = [["", ...outerKeys.map(showC)]];
    for (const ik of innerKeys) g.push([showR(ik), ...outerKeys.map((ok) => showV(value[ok]?.[ik]))]);
    return g;
  };

  const parseGrid = (g: string[][]): Table2D => {
    // (a breakpoint left empty, past the end, continues the last step, as shown)
    const asKeys = (raw: string[], read: (t: string) => number): number[] =>
      continueKeys(raw.map((s) => String(s))).map((t) => read(t.trim()));
    const outer = asKeys((g[0] ?? []).slice(1), readC);
    const inner = asKeys(g.slice(1).map((row) => row[0] ?? ""), readR);
    const result: Table2D = {};
    outer.forEach((ok) => (result[String(ok)] ??= {}));
    inner.forEach((ik, i) => {
      outer.forEach((ok, j) => {
        const cell = g[i + 1]?.[j + 1];
        const t = cell === undefined ? "" : String(cell).trim();
        if (t === "") return;
        if (Number.isFinite(Number(t))) result[String(ok)][String(ik)] = readV(t);
      });
    });
    return result;
  };

  // the corner names what the body cells hold; like a 1D table's headers, a
  // double-click (or Enter, F2) on it opens their unit's chooser
  const corner: GridCell = { text: `${valueLabel} (${units.value})`, kind: "corner", readOnly: true };
  const openCorner = (r: number, c: number) => {
    if (r !== 0 || c !== 0 || alternatives(valueUnit).length === 0) return false;
    setUnitOpen("value");
    return true;
  };
  const matrix: GridCell[][] = [
    [corner, ...outerKeys.map((k): GridCell => ({ text: showC(k), kind: "colHeader" }))],
    ...innerKeys.map((ik): GridCell[] => [
      { text: showR(ik), kind: "rowHeader" },
      ...outerKeys.map((ok): GridCell => ({ text: showV(value[ok]?.[ik]), kind: "body" })),
    ]),
  ];

  // a body cell may be left empty (no value at that point, like Delete)
  const validate = (r: number, c: number, text: string): GridIssue | null => {
    if (r === 0 && c === 0) return null;
    if (r === 0) return axisIssue(axes[0].name, "column", text, outerKeys.map(showC), c - 1);
    if (c === 0) return axisIssue(axes[1].name, "row", text, innerKeys.map(showR), r - 1);
    return text.trim() === "" ? null : numberIssue(text);
  };

  const commit = (r: number, c: number, text: string) => {
    if (r === 0 && c === 0) return;
    const g = gridText();
    g[r][c] = text;
    onChange(parseGrid(g));
  };

  const pasteBlock = (r: number, c: number, block: string[][]): string | void => {
    const g = gridText();
    const needRows = r + block.length;
    const needCols = c + Math.max(...block.map((b) => b.length));
    while (g.length < needRows) g.push([""]);
    for (const row of g) while (row.length < needCols) row.push("");
    block.forEach((brow, bi) => brow.forEach((val, bj) => {
      if (r + bi === 0 && c + bj === 0) return; // don't overwrite the corner
      g[r + bi][c + bj] = val;
    }));
    const problem =
      pastedAxisIssue(axes[0].name, g[0].slice(1), outerKeys.length) ??
      pastedAxisIssue(axes[1].name, g.slice(1).map((row) => row[0]), innerKeys.length) ??
      g
        .slice(1)
        .flatMap((row) => row.slice(1))
        .map((t) => (t.trim() === "" ? null : numberIssue(t)))
        .find(Boolean);
    if (problem) return `Paste not applied: ${problem.text}`;
    onChange(parseGrid(g));
  };

  const clearRange = (cells: { r: number; c: number }[]) => {
    const g = gridText();
    for (const { r, c } of cells) if (r >= 1 && c >= 1) g[r][c] = "";
    onChange(parseGrid(g));
  };

  const addColumn = () => {
    const nk = String(nextAxisKey(outerKeys, 50));
    const col: Record<string, number> = {};
    for (const ik of innerKeys) col[ik] = 0;
    onChange({ ...value, [nk]: col });
  };
  const addRow = () => {
    const nk = String(nextAxisKey(innerKeys, 500));
    const next: Table2D = {};
    for (const ok of outerKeys) next[ok] = { ...value[ok], [nk]: 0 };
    onChange(next);
  };
  const deleteRows = () => {
    if (!selRange) return;
    const drop = new Set(
      innerKeys.filter((_, i) => i + 1 >= Math.max(1, selRange.r0) && i + 1 <= selRange.r1),
    );
    if (drop.size === 0 || drop.size >= innerKeys.length) return;
    const next: Table2D = {};
    for (const ok of outerKeys) {
      next[ok] = {};
      for (const ik of Object.keys(value[ok] ?? {})) if (!drop.has(ik)) next[ok][ik] = value[ok][ik];
    }
    onChange(next);
  };
  const deleteCols = () => {
    if (!selRange) return;
    const keep = outerKeys.filter((_, j) => !(j + 1 >= Math.max(1, selRange.c0) && j + 1 <= selRange.c1));
    if (keep.length === 0 || keep.length === outerKeys.length) return;
    const next: Table2D = {};
    for (const ok of keep) next[ok] = value[ok];
    onChange(next);
  };

  // a right-click on a header: a breakpoint inserted midway (its points
  // interpolated, so the map is the same) or past an end, or some deleted
  const insertAt = (line: GridLine, at: number, where: "before" | "after") =>
    onChange(
      (line === "col"
        ? insertBreakpoint(value, 0, newBreakpoint(outerKeys, at - 1, where))
        : insertBreakpoint(value, 1, newBreakpoint(innerKeys, at - 1, where))) as Table2D,
    );
  const deleteAt = (line: GridLine, at: number[]) => {
    const [axis, list] = line === "col" ? [0, outerKeys] : [1, innerKeys];
    const drop = at.map((i) => list[i - 1]).filter((k) => k !== undefined);
    if (drop.length > 0 && drop.length < list.length)
      onChange(drop.reduce<Table2D>((t, k) => removeBreakpoint(t, axis, k) as Table2D, value));
  };

  // the selected cell: a value, at a column (axis 0) and a row (axis 1)
  const one = oneCell(selRange) && selRange!.r0 >= 1 && selRange!.c0 >= 1;
  const mark = one ? [selRange!.c0 - 1, selRange!.r0 - 1] : null;
  return (
    <div className="flex flex-col gap-1">
      <SpreadsheetGrid
        matrix={matrix}
        onCommit={commit}
        onPasteBlock={pasteBlock}
        onClearRange={clearRange}
        onSelectionChange={setSelRange}
        validate={validate}
        onHeaderOpen={openCorner}
        onImportText={importer.onImportText}
        shade="map"
        onInsert={insertAt}
        onDelete={deleteAt}
        focusCell={focusCell}
        caption={<TableUnits columns={columns} table={value} open={unitOpen} setOpen={setUnitOpen} set={units.set} />}
        tools={
          <>
            <span className="ss-table-tools-sep" aria-hidden />
            <button type="button" className="ss-toolbtn" onClick={addColumn} title={`Add a ${axes[0].name} column`}>
              <Columns3 size={12} aria-hidden /> Add {axes[0].name}
            </button>
            <button type="button" className="ss-toolbtn" onClick={addRow} title={`Add a ${axes[1].name} row`}>
              <Rows3 size={12} aria-hidden /> Add {axes[1].name}
            </button>
            <button
              type="button"
              className="ss-toolbtn"
              disabled={outerKeys.length <= 1}
              onClick={deleteCols}
              title="Delete the selected columns"
            >
              <Trash2 size={12} aria-hidden /> Columns
            </button>
            <button
              type="button"
              className="ss-toolbtn"
              disabled={innerKeys.length <= 1}
              onClick={deleteRows}
              title="Delete the selected rows"
            >
              <Trash2 size={12} aria-hidden /> Rows
            </button>
            {importer.tools}
          </>
        }
      />
      {importer.dialog}
      <p className="text-meta leading-tight text-[color:var(--ss-text-dim)]">{GRID_HINT}</p>
      <TablePlot
        table={value}
        axes={[
          { name: axes[0].name, lib: axes[0].unit, unit: cUnit, keys: outerKeys },
          { name: axes[1].name, lib: axes[1].unit, unit: rUnit, keys: innerKeys },
        ]}
        value={{ name: valueLabel, lib: valueUnit, unit: units.value }}
        mark={mark}
        onPick={(idx) => setFocusCell({ r: idx[1] + 1, c: idx[0] + 1 })}
      />
    </div>
  );
}

// ---- driving-task / road profiles as a spreadsheet ---------------------------
// The profile is stored as a "t:value; t:value; …" string (backend-parsed), but
// it is really a 1-D lookup, so we edit it in the same Excel-style grid.

function profileToTable(profile: string): Table1D {
  const obj: Table1D = {};
  for (const chunk of profile.replace(/\n/g, ";").split(";")) {
    const c = chunk.trim();
    if (!c) continue;
    const [ts, vs] = c.split(c.includes(":") ? ":" : ",");
    const t = Number(ts);
    const v = Number(vs);
    if (Number.isFinite(t) && Number.isFinite(v)) obj[String(t)] = v;
  }
  return obj;
}

function tableToProfile(table: Table1D): string {
  return sortedNumericKeys(table)
    .map((k) => `${k}:${table[k]}`)
    .join("; ");
}

/** Axis labels for a profile param, derived from its component (and mode). */
function profileAxes(
  defId: string,
  mode: string | undefined,
): { x: AxisDef; yLabel: string; yUnit: string } {
  if (defId === "signal.driving_task")
    return {
      x: mode === "distance" ? { name: "Distance", unit: "m" } : { name: "Time", unit: "s" },
      yLabel: "Target Speed",
      yUnit: "km/h",
    };
  if (defId === "signal.road_profile")
    return {
      x: mode === "time" ? { name: "Time", unit: "s" } : { name: "Distance", unit: "m" },
      yLabel: "Grade",
      yUnit: "%",
    };
  return { x: { name: "t", unit: "" }, yLabel: "Value", yUnit: "" };
}

function ProfileGridEditor({
  value,
  defId,
  mode,
  units,
  onChange,
}: {
  value: string;
  defId: string;
  mode: string | undefined;
  units: TableShown;
  onChange: (s: string) => void;
}) {
  const { x, yLabel, yUnit } = profileAxes(defId, mode);
  return (
    <Table1DEditor
      value={profileToTable(value)}
      axis={x}
      valueLabel={yLabel}
      valueUnit={yUnit}
      // a profile is stored as text: "the numbers are in another unit" stores it so
      units={{ ...units, set: (slot, unit, t) => units.set(slot, unit, t && tableToProfile(t as Table1D)) }}
      onChange={(t) => onChange(tableToProfile(t))}
      importTarget={{ componentDefId: defId, paramKey: "profile", mode }}
      onImport={(v) => onChange(String(v))}
    />
  );
}

/** Number field that stores only what is a number: clearing it or a half-typed
 *  value is never stored as 0, and leaving the field without a number puts the
 *  stored value back. With `def`, a number outside its limits turns it red as
 *  it is typed (it is still stored, so Data Checks say the same); the reason
 *  is shown by the caller, in the element `describedBy` names. With `onClear`,
 *  an empty field is allowed and means none (`value` null). With `unit`
 *  (another than `def`'s), the value is shown and typed in that unit, and
 *  stored in `def`'s: a field left alone writes nothing (units.ts). */
export function NumberInput({
  value,
  onChange,
  onClear,
  label,
  def,
  unit,
  describedBy,
}: {
  value: number | null;
  onChange: (v: number) => void;
  onClear?: () => void;
  label?: string;
  def?: ParameterDef;
  unit?: string;
  describedBy?: string;
}) {
  const lib = def?.unit ?? "";
  const shownIn = unit ?? lib;
  const read = (t: string) => toStored(Number(t), lib, shownIn);
  const stored = value == null ? "" : showNumber(value, lib, shownIn);
  const [text, setText] = useState(stored);
  const [shown, setShown] = useState(stored);
  // the stored value (or its unit) changed elsewhere (undo, another view): show it
  if (stored !== shown) {
    setShown(stored);
    if (read(text) !== value || text.trim() === "") setText(stored);
  }
  const empty = text.trim() === "";
  const invalid = empty ? !onClear : !Number.isFinite(Number(text));
  const outside = !invalid && !empty && def ? rangeProblem(def, read(text)) : null;
  return (
    <input
      type="number"
      className="ss-input"
      value={text}
      step="any"
      aria-label={label}
      aria-invalid={invalid || Boolean(outside) || undefined}
      aria-describedby={outside ? describedBy : undefined}
      aria-description={def?.description ?? undefined}
      title={invalid ? `Enter a number (leaving the field keeps ${stored || "it empty"})` : undefined}
      onChange={(e) => {
        setText(e.target.value);
        const t = e.target.value.trim();
        if (t === "") onClear?.();
        else if (Number.isFinite(Number(t))) onChange(read(t));
      }}
      onBlur={() => invalid && setText(stored)}
    />
  );
}

function ParameterInput({
  def,
  value,
  unit,
  onChange,
  describedBy,
}: {
  def: ParameterDef;
  value: ParamValue;
  /** a number's shown unit */
  unit?: string;
  onChange: (v: ParamValue) => void;
  describedBy?: string;
}) {
  switch (def.type) {
    case "boolean":
      return (
        <input
          type="checkbox"
          aria-label={def.label}
          aria-description={def.description ?? undefined}
          checked={Boolean(value)}
          onChange={(e) => onChange(e.target.checked)}
        />
      );
    case "enum":
      return (
        <select
          className="ss-input"
          aria-label={def.label}
          aria-description={def.description ?? undefined}
          title={String(value)}
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
        >
          {(def.options ?? []).map((o) => {
            const { label, notYet } = optionLabel(def.key, o);
            return (
              <option key={o} value={o} disabled={notYet}>
                {label}
              </option>
            );
          })}
        </select>
      );
    case "number":
      return (
        <NumberInput
          value={value == null ? null : Number(value)}
          onChange={onChange}
          label={def.label}
          def={def}
          unit={unit}
          describedBy={describedBy}
        />
      );
    case "file":
      return <FileInput def={def} value={String(value ?? "")} onChange={onChange} />;
    case "code":
      return (
        <textarea
          className="ss-input w-full resize-y font-mono text-meta"
          rows={12}
          spellCheck={false}
          aria-label={def.label}
          aria-description={def.description ?? undefined}
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
        />
      );
    default:
      return def.key === "profile" ? (
        <textarea
          className="ss-input w-full resize-y font-mono text-meta"
          rows={2}
          aria-label={def.label}
          aria-description={def.description ?? undefined}
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
        />
      ) : (
        <input
          className="ss-input w-full"
          aria-label={def.label}
          aria-description={def.description ?? undefined}
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
        />
      );
  }
}

/** A file parameter (STD-02): one of the project's attached files, or a new
 *  one attached on the spot (copied into the project's resources folder). */
function FileInput({ def, value, onChange }: { def: ParameterDef; value: string; onChange: (v: ParamValue) => void }) {
  const attachments = useProjectStore((s) => s.project?.attachments) ?? [];
  const attachFile = useProjectStore((s) => s.attachFile);
  const input = useRef<HTMLInputElement>(null);
  const accept = def.accept ?? [];
  const fits = (path: string) => accept.length === 0 || accept.some((ext) => path.toLowerCase().endsWith(ext.toLowerCase()));
  const choices = attachments.filter((a) => fits(a.path) || a.path === value);
  const known = value === "" || attachments.some((a) => a.path === value);
  return (
    <div className="flex w-full items-center gap-1">
      <select
        className="ss-input min-w-0 flex-1"
        aria-label={def.label}
        aria-description={def.description ?? undefined}
        aria-invalid={!known || undefined}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">(no file)</option>
        {!known && <option value={value}>{value} (not attached)</option>}
        {choices.map((a) => (
          <option key={a.path} value={a.path}>
            {a.path.replace(/^resources\//, "")}
          </option>
        ))}
      </select>
      <button className="ss-toolbtn shrink-0 px-1.5" title="Attach a file to the project and use it here" onClick={() => input.current?.click()}>
        Attach…
      </button>
      <input
        ref={input}
        type="file"
        className="hidden"
        aria-label={`Attach a file for ${def.label}`}
        accept={accept.join(",") || undefined}
        onChange={async (e) => {
          const el = e.target;
          const f = el.files?.[0];
          el.value = "";
          if (!f) return;
          const path = await attachFile(f);
          if (path) onChange(path);
        }}
      />
    </div>
  );
}

/** A table's outside-the-data setting per axis: the element's own, else the
 *  library's. Applies on the next run. */
function OutsideSettings({ element, param }: { element: ElementInstance; param: ParameterDef }) {
  const setTableOutside = useProjectStore((s) => s.setTableOutside);
  const axes = param.axes ?? [];
  const current = axes.map(
    (a, i) => element.tableOutside?.[param.key]?.[i] ?? a.outside ?? "clamp",
  );
  return (
    <div className="mb-1 flex flex-wrap gap-x-3 gap-y-1 text-meta text-[color:var(--ss-text-dim)]">
      {axes.map((a, i) => (
        <label key={a.name} className="flex items-center gap-1">
          Outside the {a.name} data:
          <select
            className="ss-input"
            value={current[i]}
            onChange={(e) => {
              const next = [...current];
              next[i] = e.target.value as OutsidePolicy;
              setTableOutside(element.id, param.key, next);
            }}
          >
            {(Object.keys(OUTSIDE_LABELS) as OutsidePolicy[]).map((k) => (
              <option key={k} value={k}>
                {OUTSIDE_LABELS[k]}
              </option>
            ))}
          </select>
        </label>
      ))}
    </div>
  );
}

/** Add/rename/remove per-instance signal ports (Script, Monitor). */
function DynamicPortsEditor({ element }: { element: ElementInstance }) {
  const setDynamicPorts = useProjectStore((s) => s.setDynamicPorts);
  const unitGroups = useProjectStore((s) => s.unitGroups);
  const ports = element.dynamicPorts ?? [];
  const commit = (next: PortDef[]) => setDynamicPorts(element.id, next);
  const add = (direction: "input" | "output") => {
    const base = direction === "input" ? "in" : "out";
    let n = 1;
    while (ports.some((p) => p.id === `${base}_${n}`)) n += 1;
    commit([
      ...ports,
      {
        id: `${base}_${n}`,
        name: `${base}_${n}`,
        direction,
        kind: "signal",
        unitGroup: "No Unit",
      },
    ]);
  };
  const rename = (port: PortDef, name: string) => {
    let id = slugify(name);
    while (ports.some((p) => p.id === id && p.id !== port.id)) id += "_";
    commit(ports.map((p) => (p.id === port.id ? { ...p, id, name: id } : p)));
  };
  return (
    <div>
      <div className="mb-1 flex items-center gap-2 text-meta font-semibold text-[color:var(--ss-text-dim)]">
        Signal Ports
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-meta font-normal"
          onClick={() => add("input")}
        >
          <Plus size={10} /> input
        </button>
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-meta font-normal"
          onClick={() => add("output")}
        >
          <Plus size={10} /> output
        </button>
      </div>
      {ports.length === 0 && (
        <div className="px-1 pb-1 text-meta italic text-[color:var(--ss-text-dim)]">
          No ports yet — add inputs/outputs; their names are the keys in the
          script's <code>inputs</code> / returned dict.
        </div>
      )}
      {ports.map((p) => (
        <div key={p.id} className="flex items-center gap-1 py-0.5">
          <span
            className={`rounded px-1 text-meta font-semibold uppercase ${
              p.direction === "input"
                ? "bg-[color:var(--ss-info-soft)] text-[color:var(--ss-info)]"
                : "bg-[color:var(--ss-warn-soft)] text-[color:var(--ss-warn)]"
            }`}
          >
            {p.direction === "input" ? "in" : "out"}
          </span>
          <input
            className="ss-input flex-1 font-mono text-meta"
            defaultValue={p.name}
            title="Port name (renaming disconnects existing wires)"
            onBlur={(e) => e.target.value !== p.name && rename(p, e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
          <select
            className="ss-input w-[92px] text-meta"
            aria-label={`Unit of ${p.name}`}
            title="The unit this port gives or expects. Data Checks warn when a wire joins two different units, such as a SOC in % into a port set to Fraction (0-1); nothing is converted."
            value={p.unitGroup ?? "No Unit"}
            onChange={(e) =>
              commit(ports.map((q) => (q.id === p.id ? { ...q, unitGroup: e.target.value } : q)))
            }
          >
            {Object.entries(unitGroups).map(([group, unit]) => (
              <option key={group} value={group}>
                {group === "No Unit" ? "Not set" : unit === "-" || unit === group ? group : `${group} (${unit})`}
              </option>
            ))}
          </select>
          <button
            className="text-[color:var(--ss-text-dim)] hover:text-[color:var(--ss-err)]"
            title="Remove port (disconnects its wires)"
            onClick={() => commit(ports.filter((q) => q.id !== p.id))}
          >
            <Trash2 size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}

// profiles are stored as strings but edited as a full-width grid, so they
// join the tables/code in the "big" (full-width) group rather than the
// compact scalar table.
/** A parameter's help texts (LRN-05): what it is, typical values and where
 *  to find the real number. */
function ParamHelp({ def, unit = def.unit }: { def: ParameterDef; unit?: string }) {
  return (
    <>
      <p>{def.description}</p>
      {def.typical && (
        <p>
          <b>Typical:</b> {convertProse(def.typical, def.unit, unit)}
        </p>
      )}
      {def.whereToFind && (
        <p>
          <b>Where to find it:</b> {def.whereToFind}
        </p>
      )}
    </>
  );
}

/** "Default 100 % · allowed: at least 0 and at most 200 %", in the unit the
 *  parameter is shown in */
function defaultText(def: ParameterDef, shownIn = def.unit): string | null {
  const unit = shownIn === "-" ? "" : ` ${shownIn}`;
  const limits = limitsText(def, shownIn);
  if (def.type === "number")
    return `Default ${cellText(Number(def.default), def.unit, shownIn)}${unit}${limits ? ` · allowed: ${limits}` : ""}`;
  if (def.type === "enum") return `Default ${def.default}`;
  if (def.type === "boolean") return def.default ? "Ticked by default" : "Not ticked by default";
  return null;
}

const isProfile = (p: ParameterDef) => p.type === "string" && p.key === "profile";
const isTable = (p: ParameterDef) => p.type === "table1d" || p.type === "table2d" || p.type === "table";
const isBig = (p: ParameterDef) => isTable(p) || p.type === "maps" || p.type === "code" || isProfile(p);

/** The sections a user opened or folded, by "part id/section" (U-4.9). */
function loadOpenSections(): Record<string, boolean> {
  try {
    const saved: unknown = JSON.parse(window.localStorage.getItem(PARAM_SECTIONS_KEY) ?? "{}");
    return saved && typeof saved === "object" && !Array.isArray(saved) ? (saved as Record<string, boolean>) : {};
  } catch {
    return {};
  }
}

/** "Case “City” runs with 60 %, not this part's own value": a button to that
 *  value in Cases & Parameters. */
function CaseLine({ text, onGo }: { text: string; onGo: () => void }) {
  return (
    <button type="button" className="ss-case-link" title="Show the case's value in Cases & Parameters" onClick={onGo}>
      {text}
    </button>
  );
}

export function ElementForm({
  element,
  def,
  compact = false,
}: {
  element: ElementInstance;
  def: ComponentDef;
  /** In the narrow Properties panel, table/code/profile params collapse to an
   *  "Edit…" button that opens the part's parameter window at it. */
  compact?: boolean;
}) {
  const setParameter = useProjectStore((s) => s.setParameter);
  const setDrivingCycle = useProjectStore((s) => s.setDrivingCycle);
  const setParameters = useProjectStore((s) => s.setParameters);
  const renameElement = useProjectStore((s) => s.renameElement);
  const setActiveSystem = useProjectStore((s) => s.setActiveSystem);
  const running = useProjectStore((s) => s.running);
  const openParamDialog = useUIStore((s) => s.openParamDialog);

  // a Driving Task on a drive cycle has no typed profile to edit, nor a
  // Road Profile that takes its grade from one (CON-11); the cycle gets a
  // full-width row of its own below the table
  const drivingTask = def.id === "signal.driving_task";
  const roadProfile = def.id === "signal.road_profile";
  const cycleId = String(element.parameterOverrides.cycle ?? "");
  // a parameter that applies only to one setting of another is hidden otherwise
  const shown = useMemo(() => {
    const value = (key: string) =>
      element.parameterOverrides[key] ?? def.parameters.find((q) => q.key === key)?.default;
    return (p: ParameterDef) => !p.showIf || p.showIf.values.includes(value(p.showIf.key) as ScalarValue);
  }, [def, element.parameterOverrides]);
  // an FMU block's file parameters are shown (and set) by its FMU panel
  const fmuBlock = def.id === FMU_DEF_ID;
  const scalarParams = useMemo(
    () =>
      def.parameters.filter(
        (p) =>
          !isBig(p) &&
          p.key !== "cycle" &&
          !(roadProfile && cycleId && p.key === "mode") &&
          !(fmuBlock && FMU_FILE_KEYS.includes(p.key)) &&
          shown(p),
      ),
    [def, roadProfile, cycleId, fmuBlock, shown],
  );
  const bigParams = useMemo(
    () => def.parameters.filter((p) => isBig(p) && !((drivingTask || roadProfile) && cycleId && isProfile(p)) && shown(p)),
    [def, drivingTask, roadProfile, cycleId, shown],
  );
  // (a number with no value, which the engine refuses, shows as an empty
  // field, not as the library's value it does not have)
  const valueOf = (p: ParameterDef): ParamValue =>
    p.type === "number" && element.parameterOverrides[p.key] === null
      ? (null as unknown as ParamValue)
      : (element.parameterOverrides[p.key] ?? p.default);
  const valueOfKey = (key: string): ParamValue | undefined =>
    element.parameterOverrides[key] ?? def.parameters.find((q) => q.key === key)?.default;
  // A Wheel's tyre (MOD-48): a code that reads fills in the radius and the
  // tyre estimates, a label class the rolling resistance, in one undo step
  const wheel = def.id === "propulsion.wheel";
  const tyre = wheel ? parseTyreCode(String(valueOfKey("tyre_code") ?? "")) : null;
  const radiusFactor = Number(valueOfKey("rolling_radius_factor") ?? 0.97) || 0.97;
  const change = (key: string, v: ParamValue) => {
    if (wheel && (key === "tyre_code" || key === "rolling_radius_factor")) {
      const spec = key === "tyre_code" ? parseTyreCode(String(v)) : tyre;
      if (spec) {
        const values = tyreValues(spec, key === "rolling_radius_factor" ? Number(v) || radiusFactor : radiusFactor);
        setParameters(element.id, key === "tyre_code" ? { ...values, [key]: v } : { [key]: v, radius_m: values.radius_m });
        return;
      }
    }
    if (wheel && key === "tyre_label_class" && LABEL_RRC[String(v)] !== undefined) {
      setParameters(element.id, { [key]: v, rolling_resistance: LABEL_RRC[String(v)] / 1000 });
      return;
    }
    setParameter(element.id, key, v);
  };
  // the unit each value is shown in (U1-U4): the part's choice, kept in the
  // project with it; the values themselves stay in the library's units
  const setDisplayUnit = useProjectStore((s) => s.setDisplayUnit);
  const unitOf = (p: ParameterDef, slot: UnitSlot = "value") =>
    shownUnit(element, p.key, slot, slot === "value" ? p.unit : (p.axes?.[slot]?.unit ?? ""));
  const keepUnit = (key: string, lib: string, slot: UnitSlot, unit: string, value?: ParamValue) =>
    setDisplayUnit(element.id, key, slot, unit === lib ? null : unit, value);
  const tableShown = (p: ParameterDef, axes = p.axes ?? [], valueLib = p.unit): TableShown => ({
    axes: axes.map((a, i) => shownUnit(element, p.key, i, a.unit)),
    value: shownUnit(element, p.key, "value", valueLib),
    set: (slot, unit, table) =>
      keepUnit(p.key, slot === "value" ? valueLib : axes[slot].unit, slot, unit, table as ParamValue | undefined),
  });
  // the number whose unit chooser is open, and the unit controls to go back to
  const [unitOpen, setUnitOpen] = useState<string | null>(null);
  const unitButtons = useRef(new Map<string, HTMLButtonElement | null>());
  const closeUnits = (key: string) => {
    setUnitOpen(null);
    unitButtons.current.get(key)?.focus();
  };
  const problemOf = (p: ParameterDef) =>
    p.type === "number" && valueOf(p) != null ? rangeProblem(p, Number(valueOf(p)), unitOf(p)) : null;
  // the dialog and Properties can show the same part: ids must not clash
  const formId = useId();

  // the help card (UX-10): one per form, beside the parameter the pointer
  // rests on (after 300 ms) or the one with focus (at once); it stays while
  // the pointer is on it or focus is in its row, and Esc closes it.
  // `pointed` and `focused` are the rows under the pointer and with focus
  // (the card counts as its row's); `help` is what the card shows.
  const [help, setHelp] = useState<string | null>(null);
  const [pointed, setPointed] = useState<string | null>(null);
  const [focused, setFocused] = useState<string | null>(null);
  const want = pointed ?? focused;
  useEffect(() => {
    if (want === help) return;
    const t = setTimeout(() => setHelp(want), want && want !== focused ? 300 : 150);
    return () => clearTimeout(t);
  }, [want, help, focused]);
  const card = useRef<HTMLDivElement>(null);
  const win = useOwnerWindow();
  // a dialog over Properties hides its card (the focus can stay behind it)
  const covered = useUIStore((s) => compact && (s.paramDialogId !== null || s.dialog !== null));
  // (a unit chooser open under its row: the card steps aside, so it never
  // lies over the chooser in a narrow window)
  const helpDef = covered || unitOpen ? undefined : def.parameters.find((p) => p.key === help && p.description);
  useEffect(() => {
    const el = card.current;
    if (!el?.showPopover) return; // no popovers in jsdom
    const open = el.matches(":popover-open");
    if (helpDef && !open) el.showPopover();
    else if (!helpDef && open) el.hidePopover();
  }, [helpDef]);
  // Esc closes the card, before it can close a dialog or anything else
  useEffect(() => {
    if (!helpDef) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // an open unit chooser takes its own Esc
      if ((e.target as Element | null)?.closest?.(".ss-unit-chooser")) return;
      e.stopPropagation();
      setHelp(null);
      setPointed(null);
      setFocused(null);
    };
    win.addEventListener("keydown", onKey, true);
    return () => win.removeEventListener("keydown", onKey, true);
  }, [helpDef, win]);
  const helpPage = (p: ParameterDef) => `${componentHelpPage(def.id)}#${p.key}`;
  // a different anchor for the dialog's card and the Properties panel's
  const anchor = compact ? "--ss-help-panel" : "--ss-help-dialog";
  const helpProps = (p: ParameterDef) => ({
    "data-help": helpPage(p), // F1 opens it
    style: help === p.key ? { anchorName: anchor } : undefined,
    onMouseEnter: () => setPointed(p.key),
    onMouseLeave: () => setPointed(null),
    onFocus: () => {
      setFocused(p.key);
      setHelp(p.key);
    },
    // moving to the card's own button keeps it
    onBlur: (e: React.FocusEvent) => e.relatedTarget?.closest(".ss-help-card") || setFocused(null),
  });

  // the window's search and Changed only (U-4.9): a parameter shows when its
  // label or description holds the words typed and, with Changed only, when
  // its value is not the library's. Not in the narrow Properties panel.
  const [query, setQuery] = useState("");
  const [changedOnly, setChangedOnly] = useState(false);
  const q = compact ? "" : query.trim().toLowerCase();
  const only = !compact && changedOnly;
  const filtering = q !== "" || only;
  const isChanged = (p: ParameterDef) =>
    Object.hasOwn(element.parameterOverrides, p.key) &&
    JSON.stringify(element.parameterOverrides[p.key]) !== JSON.stringify(p.default);
  const matches = (p: ParameterDef) =>
    (!q || p.label.toLowerCase().includes(q) || (p.description ?? "").toLowerCase().includes(q)) &&
    (!only || isChanged(p));

  // a dot beside a value that is not the library's; a click puts it back,
  // one undo step (U-4.9). None in the narrow panel, nor on an unchanged value.
  const resetParameter = useProjectStore((s) => s.resetParameter);
  const libText = (p: ParameterDef) => {
    const unit = unitOf(p);
    if (p.type === "number" && typeof p.default === "number")
      return `${showNumber(p.default, p.unit, unit)}${unit === "-" ? "" : ` ${unit}`}`;
    if (p.type === "boolean") return p.default ? "ticked" : "not ticked";
    if (p.type === "enum" || (p.type === "string" && !isProfile(p))) return String(p.default) || "empty";
    return null; // a table, a profile, code: too long to quote
  };
  const dot = (p: ParameterDef, inline = false) => {
    if (compact || !isChanged(p)) return null;
    const lib = libText(p);
    return (
      <button
        type="button"
        className={`ss-changed-dot${inline ? " ss-changed-dot-inline" : ""}`}
        title={`Changed: click to put back the library's ${lib ?? "value"}`}
        aria-label={`Put ${paramName(p)} back to the library's ${lib ?? "value"}`}
        onClick={() => resetParameter(element.id, p.key)}
      />
    );
  };

  // the active case runs with its own value of a parameter (U-4.10): a line
  // there says so, in the unit shown; the field still sets the part's own
  // (a project from an old file may have no cases)
  const activeCase = useProjectStore((s) => s.project?.cases?.find((c) => c.id === s.activeCaseId));
  const cycles = useProjectStore((s) => s.cycles);
  const caseNote = (p: ParameterDef): string | null => {
    if (compact || !activeCase) return null;
    const v = activeCase.parameterOverrides?.[element.id]?.[p.key];
    if (v === undefined || JSON.stringify(v) === JSON.stringify(valueOf(p))) return null;
    const unit = unitOf(p);
    const text =
      p.key === "cycle"
        ? (cycles.find((c) => c.id === v)?.name ?? (v ? String(v) : "the typed profile"))
        : typeof v === "number"
          ? `${showNumber(v, p.unit, unit)}${unit === "-" ? "" : ` ${unit}`}`
          : typeof v === "boolean"
            ? v ? "this ticked" : "this not ticked"
            : String(v);
    return `Case “${activeCase.name}” runs with ${text}, not this part's own value.`;
  };

  // the window's sections (U-4.9): the library's headings, the last one
  // (rarely used values) folded at first; folded or open is kept per user.
  // While a search or Changed only is on, the sections with a match are open.
  const sections = compact || !def.sections?.length ? null : def.sections;
  const [opened, setOpened] = useState<Record<string, boolean>>(loadOpenSections);
  const sectionKey = (name: string) => `${def.id}/${name}`;
  const toggleSection = (name: string, open: boolean) => {
    if (filtering || opened[sectionKey(name)] === open) return;
    const next = { ...opened, [sectionKey(name)]: open };
    setOpened(next);
    try {
      window.localStorage.setItem(PARAM_SECTIONS_KEY, JSON.stringify(next));
    } catch {
      /* storage unavailable: this window only */
    }
  };
  const profile = drivingTask ? profileToTable(String(valueOf(def.parameters.find(isProfile)!))) : {};
  // (a Road Profile from a file older than CON-11 has a catalogue with it)
  const cycleDef = def.parameters.find((p) => p.key === "cycle")!;
  const profileMode = String(valueOfKey("mode") ?? "");

  const scalarRow = (p: ParameterDef) => {
    const problem = problemOf(p);
    const unit = unitOf(p);
    const choosable = p.type === "number" && alternatives(p.unit).length > 0;
    return (
      <Fragment key={p.key}>
        <tr {...helpProps(p)} data-param={p.key}>
          {/* the full label is in the help card, or else its tooltip */}
          <td className="ss-td flex items-center text-meta" title={p.description ? undefined : p.label}>
            <span className="truncate">{p.label}</span>
            <SourceBadge src={element.parameterSources?.[p.key]} />
            {running && p.variability === "fixed" && (
              <span
                className="ml-1 shrink-0 text-meta italic text-[color:var(--ss-text-dim)]"
                title="Structural parameter — a live edit takes effect on the next run"
              >
                (next run)
              </span>
            )}
          </td>
          <td className="ss-td relative">
            {dot(p)}
            <ParameterInput
              def={p}
              value={valueOf(p)}
              unit={unit}
              describedBy={`${formId}${p.key}-problem`}
              onChange={(v) => change(p.key, v)}
            />
          </td>
          <td className="ss-td whitespace-nowrap text-meta text-[color:var(--ss-text-dim)]">
            {choosable ? (
              <UnitButton
                what={paramName(p)}
                unit={unit}
                open={unitOpen === p.key}
                onOpen={() => setUnitOpen(p.key)}
                buttonRef={(el) => void unitButtons.current.set(p.key, el)}
              />
            ) : (
              p.unit
            )}
          </td>
        </tr>
        {choosable && unitOpen === p.key && (
          <tr>
            <td className="ss-td ss-unit-row">
              <UnitChooser
                what={paramName(p)}
                lib={p.unit}
                unit={unit}
                onShow={(u) => {
                  keepUnit(p.key, p.unit, "value", u);
                  closeUnits(p.key);
                }}
                onReinterpret={(u) => {
                  keepUnit(p.key, p.unit, "value", u, reinterpret(Number(valueOf(p)), p.unit, unit, u));
                  closeUnits(p.key);
                }}
                onClose={() => closeUnits(p.key)}
              />
            </td>
          </tr>
        )}
        {problem && (
          // why the value is outside its limits, across the whole row
          <tr>
            <td id={`${formId}${p.key}-problem`} className="ss-td ss-param-problem">
              <span role="alert">
                {paramName(p)} {problem}.
              </span>
            </td>
          </tr>
        )}
        {caseNote(p) && (
          // the active case runs with its own value: say so, and which one the field sets
          <tr>
            <td className="ss-td ss-case-line">
              <CaseLine text={caseNote(p)!} onGo={() => showCaseValue(element.id, p.key)} />
            </td>
          </tr>
        )}
      </Fragment>
    );
  };

  // the value and unit take the width they need; the label gets the rest (at
  // least 72 px), on one line, with the full text in its tooltip. Under a
  // section's heading the column names are for screen readers only.
  const scalarTable = (list: ParameterDef[], heading = true) =>
    list.length > 0 && (
      <table className="ss-param-grid w-full">
        <thead className={heading ? undefined : "sr-only"}>
          <tr>
            <th className="ss-th truncate">Parameter</th>
            <th className="ss-th">Value</th>
            <th className="ss-th">Unit</th>
          </tr>
        </thead>
        <tbody>{list.map(scalarRow)}</tbody>
      </table>
    );

  const bigParam = (p: ParameterDef) => (
    <div key={p.key} data-param={p.key}>
      <div className="mb-1 text-meta font-semibold text-[color:var(--ss-text-dim)]">
        {isProfile(p) ? "Profile" : p.label}
        {!isProfile(p) && p.type !== "code" && p.type !== "maps" ? ` (${unitOf(p)})` : ""}
        {running && !isProfile(p) && (
          <span className="ml-1 font-normal italic">— applies on next run</span>
        )}
        {dot(p, true)}
      </div>
      {p.description && (
        <div className="ss-param-help mb-1 text-meta text-[color:var(--ss-text-dim)]">
          <ParamHelp def={p} unit={unitOf(p)} />
        </div>
      )}
      {p.axes?.some((a) => a.outside) && <OutsideSettings element={element} param={p} />}
      {isProfile(p) ? (
        <ProfileGridEditor
          value={String(valueOf(p))}
          defId={def.id}
          mode={profileMode}
          units={tableShown(p, [profileAxes(def.id, profileMode).x], profileAxes(def.id, profileMode).yUnit)}
          onChange={(v) => setParameter(element.id, p.key, v)}
        />
      ) : isTable(p) && p.axes?.length === 1 ? (
        <Table1DEditor
          value={valueOf(p) as Table1D}
          axis={p.axes[0]}
          valueLabel={p.label}
          valueUnit={p.unit}
          units={tableShown(p)}
          onChange={(v) => setParameter(element.id, p.key, v)}
          importTarget={{ componentDefId: def.id, paramKey: p.key }}
        />
      ) : isTable(p) && p.axes?.length === 2 ? (
        <Table2DEditor
          value={valueOf(p) as Table2D}
          axes={[p.axes[0], p.axes[1]]}
          valueLabel={p.label}
          valueUnit={p.unit}
          units={tableShown(p)}
          onChange={(v) => setParameter(element.id, p.key, v)}
          importTarget={{ componentDefId: def.id, paramKey: p.key }}
        />
      ) : isTable(p) && (p.axes?.length ?? 0) > 2 ? (
        // a table of three or more axes: one grid, two axes shown (docs/spec/tables.md)
        <LibraryTableN
          target={{ componentDefId: def.id, paramKey: p.key }}
          axes={p.axes!.map((a, i) => ({ name: a.name, lib: a.unit, unit: unitOf(p, i) }))}
          outputs={[{ key: "value", name: p.label, lib: p.unit, unit: unitOf(p) }]}
          tables={{ value: valueOf(p) as TableN }}
          onChange={(t) => setParameter(element.id, p.key, t.value)}
          setUnit={(slot, _out, unit, t) =>
            keepUnit(p.key, slot === "value" ? p.unit : p.axes![slot].unit, slot, unit, t?.value)
          }
        />
      ) : p.type === "maps" ? (
        <MapEditor element={element} />
      ) : (
        <ParameterInput
          def={p}
          value={valueOf(p)}
          onChange={(v) => setParameter(element.id, p.key, v)}
        />
      )}
    </div>
  );

  const tyreSummary = (list: ParameterDef[]) =>
    tyre &&
    list.some((p) => p.key === "tyre_code") && (
      <p className="text-meta text-[color:var(--ss-text-dim)]" data-testid="tyre-summary">
        {describeTyre(tyre, radiusFactor)}
      </p>
    );
  const section = (s: { name: string; folded?: boolean }) => {
    const list = scalarParams.filter((p) => p.section === s.name && matches(p));
    const bigs = bigParams.filter((p) => p.section === s.name && matches(p));
    if (!list.length && !bigs.length) return null;
    const open = filtering || (opened[sectionKey(s.name)] ?? !s.folded);
    return (
      <details
        key={s.name}
        className="ss-param-section"
        open={open}
        onToggle={(e) => toggleSection(s.name, e.currentTarget.open)}
      >
        {/* a click (or Enter, Space) folds or opens it and keeps that at once:
            the browser's own toggle event comes a moment later, and a window
            closed in that moment forgot what was just opened */}
        <summary
          onClick={(e) => {
            e.preventDefault();
            toggleSection(s.name, !open);
          }}
        >
          {s.name}
        </summary>
        <div className="flex flex-col gap-2 pt-1">
          {scalarTable(list, false)}
          {tyreSummary(list)}
          {bigs.map(bigParam)}
        </div>
      </details>
    );
  };
  // what a search or Changed only found, and what to say when it found nothing
  const found =
    scalarParams.filter(matches).length +
    (compact ? 0 : bigParams.filter(matches).length) +
    ((drivingTask || (roadProfile && cycleDef)) && cycleDef && matches(cycleDef) ? 1 : 0);
  const nothingText = q
    ? `No ${only ? "changed " : ""}parameter matches “${query.trim()}”.`
    : "No value here differs from the library's.";

  return (
    <div className="flex flex-col gap-2 p-2">
      {!compact && (
        <div className="ss-param-tools">
          <input
            type="search"
            className="ss-input min-w-0 flex-1"
            placeholder="Search parameters"
            aria-label="Search parameters"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <label className="flex shrink-0 items-center gap-1 text-meta">
            <input type="checkbox" checked={changedOnly} onChange={(e) => setChangedOnly(e.target.checked)} />
            Changed only
          </label>
        </div>
      )}
      <div className="flex items-center gap-2">
        <span className="w-[72px] shrink-0 text-meta text-[color:var(--ss-text-dim)]">Name</span>
        <input
          className="ss-input min-w-0 flex-1"
          aria-label="Name"
          value={element.label}
          onChange={(e) => renameElement(element.id, e.target.value)}
        />
      </div>
      <div className="flex items-center gap-2 text-meta text-[color:var(--ss-text-dim)]">
        <span className="w-[72px] shrink-0">Type</span>
        <span className="truncate text-[color:var(--ss-text)]" title={def.name}>
          {def.name}
        </span>
        {running && (
          <span
            className="flex items-center gap-1 rounded bg-[color:var(--ss-ok-soft)] px-1.5 py-0.5 text-meta font-semibold text-[color:var(--ss-ok)]"
            title="Simulation running — number/switch edits apply immediately; tables and code apply on the next run"
          >
            <Radio size={10} /> LIVE
          </span>
        )}
        <span className="ml-auto rounded bg-[color:var(--ss-panel-alt)] px-1.5 py-0.5 text-meta uppercase">
          {def.domain}
        </span>
      </div>
      {element.isSubSystem && element.subSystemId && (
        <button
          className="ss-toolbtn justify-center border border-[color:var(--ss-border)]"
          onClick={() => setActiveSystem(element.subSystemId!)}
        >
          <CornerDownRight size={13} /> Open sub-system
        </button>
      )}
      {!filtering &&
        def.presets?.map((preset) => (
        <div key={preset.name} className="flex flex-col gap-0.5">
          <button
            className="ss-toolbtn justify-center border border-[color:var(--ss-border)]"
            onClick={() => setParameters(element.id, preset.values)}
            title={preset.note}
          >
            Apply preset: {preset.name}
          </button>
          {/* the source of the values, in full in the tooltip */}
          {preset.note && (
            <p
              className={`text-meta text-[color:var(--ss-text-dim)] ${compact ? "line-clamp-2" : ""}`}
              title={preset.note}
            >
              {preset.note}
            </p>
          )}
        </div>
      ))}
      {sections ? (
        // one value and one unit column for every section, so they line up
        // down the window: as wide as the longest value and unit shown
        <div
          className="ss-param-sections flex flex-col gap-2"
          style={
            {
              "--ss-value-col": `max(5.5rem, calc(${Math.max(
                0,
                ...scalarParams.map((p) => {
                  const v = valueOf(p);
                  return p.type === "number" && typeof v === "number" ? showNumber(v, p.unit, unitOf(p)).length : 0;
                }),
              )}ch + 24px))`,
              "--ss-unit-col": `calc(${Math.max(1, ...scalarParams.map((p) => unitOf(p).length))}ch + 26px)`,
            } as React.CSSProperties
          }
        >
          {sections.map(section)}
        </div>
      ) : (
        scalarTable(scalarParams.filter(matches))
      )}
      {!sections && tyreSummary(scalarParams.filter(matches))}
      {!filtering && <ValueSources element={element} def={def} />}
      {(drivingTask || (roadProfile && cycleDef)) && matches(cycleDef) && (
        <div className="flex flex-col gap-1" {...helpProps(cycleDef)}>
          <label className="flex flex-col gap-0.5 text-meta text-[color:var(--ss-text-dim)]">
            <span>
              {cycleDef.label}
              {running && (
                <span className="ml-1 text-meta italic" title="Structural parameter — a live edit takes effect on the next run">
                  (next run)
                </span>
              )}
            </span>
            <CycleSelect
              value={cycleId}
              label={cycleDef.label}
              description={cycleDef.description}
              gradeOnly={roadProfile}
              onChange={(v) => (drivingTask ? setDrivingCycle(element.id, v) : setParameter(element.id, "cycle", v))}
            />
          </label>
          {(drivingTask || cycleId) && (
            <CyclePreview
              cycleId={cycleId}
              points={sortedNumericKeys(profile).map((t) => [Number(t), profile[t]])}
              byDistance={
                drivingTask && String(valueOf(def.parameters.find((q) => q.key === "mode")!)) === "distance"
              }
            />
          )}
          {caseNote(cycleDef) && (
            <p className="ss-case-line">
              <CaseLine text={caseNote(cycleDef)!} onGo={() => showCaseValue(element.id, "cycle")} />
            </p>
          )}
        </div>
      )}
      {compact && bigParams.length > 0 && (
        <div className="flex flex-col gap-1">
          {bigParams.map((p) => (
            <button
              key={p.key}
              {...helpProps(p)}
              className="ss-toolbtn justify-between border border-[color:var(--ss-border)] px-2 py-1"
              aria-description={p.description ?? undefined}
              onClick={() => {
                // the card is not wanted back when the window closes
                setHelp(null);
                setPointed(null);
                setFocused(null);
                openParamDialog(element.id, p.key);
              }}
              title={p.description ? undefined : `${isProfile(p) ? "Profile" : p.label}: open the full editor in the part's window`}
            >
              <span className="flex min-w-0 items-center gap-1.5">
                {p.type === "code" ? <Pencil size={12} /> : <Table2 size={12} />}
                <span className="truncate">{isProfile(p) ? "Profile" : p.label}</span>
                {p.type !== "code" && p.type !== "maps" && !isProfile(p) ? (
                  <span className="shrink-0 text-meta text-[color:var(--ss-text-dim)]">({unitOf(p)})</span>
                ) : null}
              </span>
              <span className="shrink-0 text-meta text-[color:var(--ss-accent)]">Edit…</span>
            </button>
          ))}
        </div>
      )}
      {!compact && !sections && bigParams.filter(matches).map(bigParam)}
      {filtering ? (
        found === 0 && (
          <p role="status" className="px-1 py-2 text-body text-[color:var(--ss-text-dim)]">
            {nothingText}
          </p>
        )
      ) : fmuBlock ? (
        <FmuPanel element={element} compact={compact} />
      ) : (
        def.allowDynamicPorts && def.id !== MAP_DEF_ID && <DynamicPortsEditor element={element} />
      )}
      {!filtering && def.ports.length > 0 && (
        <div>
          <div className="mb-1 text-meta font-semibold text-[color:var(--ss-text-dim)]">
            Ports
          </div>
          {def.ports.map((p) => (
            <div key={p.id} className="flex items-center gap-2 py-0.5 text-meta">
              <span
                className="h-2 w-2 shrink-0 rounded-full"
                style={{ background: KIND_COLOR[p.kind] }}
              />
              <span className="truncate" title={p.name}>
                {p.name}
              </span>
              <span className="ml-auto shrink-0 whitespace-nowrap text-meta text-[color:var(--ss-text-dim)]">
                {p.kind} · {p.direction}
              </span>
            </div>
          ))}
        </div>
      )}
      <div
        ref={card}
        // manual: a click in the row it explains must not close it, as an
        // "auto" popover's light dismiss would
        popover="manual"
        className="ss-help-card"
        style={{ positionAnchor: anchor }}
        data-help={helpDef ? helpPage(helpDef) : undefined} // F1 opens what it shows
        onMouseEnter={() => setPointed(help)}
        onMouseLeave={() => setPointed(null)}
        // Tab out of its button closes it
        onBlur={(e) => e.currentTarget.contains(e.relatedTarget) || setFocused(null)}
      >
        {helpDef && (
          <>
            <div className="font-semibold">
              {helpDef.label}
              {helpDef.unit !== "-" && <span className="font-normal"> [{unitOf(helpDef)}]</span>}
            </div>
            <ParamHelp def={helpDef} unit={unitOf(helpDef)} />
            {defaultText(helpDef, unitOf(helpDef)) && (
              <p className="text-[color:var(--ss-text-dim)]">{defaultText(helpDef, unitOf(helpDef))}</p>
            )}
            <p>
              <button
                className="text-[color:var(--ss-accent)] underline"
                // the Properties panel's in the Help panel; a part's window's in the Help window
                onClick={() => (compact ? openHelp(helpPage(helpDef)) : openHelpWindow(helpPage(helpDef), win))}
              >
                More in the help (F1)
              </button>
            </p>
          </>
        )}
      </div>
      {!filtering && def.description && (
        <p className="border-t border-[color:var(--ss-border)] pt-2 text-meta italic text-[color:var(--ss-text-dim)]">
          {def.description}
        </p>
      )}
    </div>
  );
}

export function PropertiesPanel() {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const selectedElementId = useProjectStore((s) => s.selectedElementId);
  const selected = project?.systems
    .flatMap((s) => s.elements)
    .find((e) => e.id === selectedElementId);
  const selectedDef = selected ? libraryById[selected.componentDefId] : undefined;

  return (
    // data-properties-panel: Enter on the diagram puts the focus in here (UX-19)
    <div className="flex h-full flex-col" data-properties-panel>
      <div className="ss-panel-toolbar text-meta font-semibold">
        {selected ? `Parameters — ${selected.label}` : "Parameters"}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {selected && selectedDef ? (
          <ElementForm element={selected} def={selectedDef} compact />
        ) : (
          <div className="flex flex-col items-center gap-2 p-6 text-center text-body text-[color:var(--ss-text-dim)]">
            <Box size={22} strokeWidth={1.2} />
            Select an element on the canvas or in the Elements panel to edit its parameters.
          </div>
        )}
      </div>
    </div>
  );
}
