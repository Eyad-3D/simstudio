import { useMemo, useState } from "react";
import { Box, Columns3, CornerDownRight, Pencil, Plus, Radio, Rows3, Table2, Trash2 } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { SpreadsheetGrid, type GridCell, type GridIssue, type GridRange } from "../SpreadsheetGrid";
import type {
  AxisDef,
  ComponentDef,
  ElementInstance,
  ParameterDef,
  ParamValue,
  PortDef,
  Table1D,
  Table2D,
} from "../../types";

function sortedNumericKeys(obj: Record<string, unknown>): string[] {
  return Object.keys(obj).sort((a, b) => Number(a) - Number(b));
}

/** Next axis value after `keys`, extrapolating the last step (fallback if flat). */
function nextAxisKey(keys: string[], fallback: number): number {
  const last = keys.length ? Number(keys[keys.length - 1]) : 0;
  const prev = keys.length > 1 ? Number(keys[keys.length - 2]) : last - fallback;
  const step = keys.length > 1 ? last - prev : fallback;
  return last + (step || fallback);
}

const GRID_HINT = "Click a cell to edit · drag or Shift+click to select a range · Ctrl+C / Ctrl+V to copy & paste from Excel";

/** Why `text` cannot be stored as a number, or null when it can. */
function numberIssue(text: string): GridIssue | null {
  const t = text.trim();
  if (Number.isFinite(Number(t))) return null;
  const hint = /^-?\d+,\d+$/.test(t) ? " (use a point for decimals)" : "";
  return { level: "error", text: `'${t}' is not a number${hint}.` };
}

/** Check a typed breakpoint against the others on its axis (`keys`, in
 *  order; `index` is its own position): it must be a new number, and one out
 *  of order is flagged because the row or column will move. */
function axisIssue(
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
function pastedAxisIssue(name: string, cells: string[], existing: number): GridIssue | null {
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

/** Excel-style editable grid for a 1D lookup table {x: y}. */
function Table1DEditor({
  value,
  axis,
  valueLabel,
  valueUnit,
  onChange,
}: {
  value: Table1D;
  axis: AxisDef;
  valueLabel: string;
  valueUnit: string;
  onChange: (v: Table1D) => void;
}) {
  const [selRange, setSelRange] = useState<GridRange | null>(null);
  const keys = sortedNumericKeys(value);
  const pairs: [string, string][] = keys.map((k) => [k, String(value[k])]);

  const rebuild = (rows: [string, string][]): Table1D => {
    const obj: Table1D = {};
    for (const [x, y] of rows) {
      const xs = String(x).trim();
      const nx = Number(xs);
      if (xs === "" || !Number.isFinite(nx)) continue;
      const ny = Number(String(y).trim());
      obj[String(nx)] = Number.isFinite(ny) ? ny : 0;
    }
    return obj;
  };

  const matrix: GridCell[][] = [
    [
      { text: `${axis.name} (${axis.unit})`, kind: "colHeader", readOnly: true },
      { text: `${valueLabel} (${valueUnit})`, kind: "colHeader", readOnly: true },
    ],
    ...pairs.map(
      ([x, y]): GridCell[] => [
        { text: x, kind: "body" },
        { text: y, kind: "body" },
      ],
    ),
  ];

  const validate = (r: number, c: number, text: string): GridIssue | null => {
    if (r === 0) return null;
    if (c === 0) return axisIssue(axis.name, "row", text, keys, r - 1);
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
    block.forEach((brow, bi) => {
      brow.forEach((val, bj) => {
        const tc = startCol + bj;
        if (tc > 1) return;
        const tr = startRow + bi;
        while (rows.length <= tr) {
          const lastX = rows.length ? Number(rows[rows.length - 1][0]) : 0;
          rows.push([String((Number.isFinite(lastX) ? lastX : 0) + 1), "0"]);
        }
        rows[tr][tc] = val;
      });
    });
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

  return (
    <div className="flex flex-col gap-1">
      <SpreadsheetGrid
        matrix={matrix}
        onCommit={commit}
        onPasteBlock={pasteBlock}
        onClearRange={clearRange}
        onSelectionChange={setSelRange}
        validate={validate}
      />
      <div className="flex items-center gap-1">
        <button className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px]" onClick={addRow}>
          <Plus size={12} /> Add row
        </button>
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px] disabled:opacity-30"
          disabled={pairs.length <= 1}
          onClick={deleteRows}
          title="Delete the selected row(s)"
        >
          <Trash2 size={12} /> Delete row(s)
        </button>
      </div>
      <p className="text-[10px] leading-tight text-[color:var(--ss-text-dim)]">{GRID_HINT}</p>
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
  onChange,
}: {
  value: Table2D;
  axes: [AxisDef, AxisDef];
  valueLabel: string;
  valueUnit: string;
  onChange: (v: Table2D) => void;
}) {
  const [selRange, setSelRange] = useState<GridRange | null>(null);
  const outerKeys = sortedNumericKeys(value);
  const innerSet = new Set<string>();
  for (const ok of outerKeys) for (const ik of Object.keys(value[ok] ?? {})) innerSet.add(ik);
  const innerKeys = [...innerSet].sort((a, b) => Number(a) - Number(b));

  /** Current grid as strings: row 0 = ["", ...outer], each row = [inner, ...values]. */
  const gridText = (): string[][] => {
    const g: string[][] = [["", ...outerKeys]];
    for (const ik of innerKeys) {
      g.push([ik, ...outerKeys.map((ok) => {
        const v = value[ok]?.[ik];
        return v === undefined ? "" : String(v);
      })]);
    }
    return g;
  };

  const parseGrid = (g: string[][]): Table2D => {
    const asKeys = (raw: string[]): number[] => {
      let max = Math.max(0, ...raw.map((s) => Number(String(s).trim())).filter(Number.isFinite));
      return raw.map((s) => {
        const t = String(s).trim();
        const n = Number(t);
        if (t !== "" && Number.isFinite(n)) return n;
        max += 1;
        return max;
      });
    };
    const outer = asKeys((g[0] ?? []).slice(1));
    const inner = asKeys(g.slice(1).map((row) => row[0] ?? ""));
    const result: Table2D = {};
    outer.forEach((ok) => (result[String(ok)] ??= {}));
    inner.forEach((ik, i) => {
      outer.forEach((ok, j) => {
        const cell = g[i + 1]?.[j + 1];
        const t = cell === undefined ? "" : String(cell).trim();
        if (t === "") return;
        const v = Number(t);
        if (Number.isFinite(v)) result[String(ok)][String(ik)] = v;
      });
    });
    return result;
  };

  const corner: GridCell = { text: `${axes[1].name}\\${axes[0].name}`, kind: "corner", readOnly: true };
  const matrix: GridCell[][] = [
    [corner, ...outerKeys.map((k): GridCell => ({ text: k, kind: "colHeader" }))],
    ...innerKeys.map((ik): GridCell[] => [
      { text: ik, kind: "rowHeader" },
      ...outerKeys.map((ok): GridCell => {
        const v = value[ok]?.[ik];
        return { text: v === undefined ? "" : String(v), kind: "body" };
      }),
    ]),
  ];

  // a body cell may be left empty (no value at that point, like Delete)
  const validate = (r: number, c: number, text: string): GridIssue | null => {
    if (r === 0 && c === 0) return null;
    if (r === 0) return axisIssue(axes[0].name, "column", text, outerKeys, c - 1);
    if (c === 0) return axisIssue(axes[1].name, "row", text, innerKeys, r - 1);
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

  return (
    <div className="flex flex-col gap-1">
      <div className="text-[10px] text-[color:var(--ss-text-dim)]">
        Columns: {axes[0].name} ({axes[0].unit}) · Rows: {axes[1].name} ({axes[1].unit}) · Values:{" "}
        {valueLabel} ({valueUnit})
      </div>
      <SpreadsheetGrid
        matrix={matrix}
        onCommit={commit}
        onPasteBlock={pasteBlock}
        onClearRange={clearRange}
        onSelectionChange={setSelRange}
        validate={validate}
      />
      <div className="flex flex-wrap items-center gap-1">
        <button className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px]" onClick={addColumn} title={`Add a ${axes[0].name} column`}>
          <Columns3 size={12} /> Add {axes[0].name}
        </button>
        <button className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px]" onClick={addRow} title={`Add a ${axes[1].name} row`}>
          <Rows3 size={12} /> Add {axes[1].name}
        </button>
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px] disabled:opacity-30"
          disabled={outerKeys.length <= 1}
          onClick={deleteCols}
          title="Delete the selected column(s)"
        >
          <Trash2 size={12} /> Col(s)
        </button>
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px] disabled:opacity-30"
          disabled={innerKeys.length <= 1}
          onClick={deleteRows}
          title="Delete the selected row(s)"
        >
          <Trash2 size={12} /> Row(s)
        </button>
      </div>
      <p className="text-[10px] leading-tight text-[color:var(--ss-text-dim)]">{GRID_HINT}</p>
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
    return { x: { name: "Time", unit: "s" }, yLabel: "Target Speed", yUnit: "km/h" };
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
  onChange,
}: {
  value: string;
  defId: string;
  mode: string | undefined;
  onChange: (s: string) => void;
}) {
  const { x, yLabel, yUnit } = profileAxes(defId, mode);
  return (
    <Table1DEditor
      value={profileToTable(value)}
      axis={x}
      valueLabel={yLabel}
      valueUnit={yUnit}
      onChange={(t) => onChange(tableToProfile(t))}
    />
  );
}

/** Number field that stores only what is a number: clearing it or a half-typed
 *  value is never stored as 0, and leaving the field without a number puts the
 *  stored value back. */
function NumberInput({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const [text, setText] = useState(String(value));
  const [shown, setShown] = useState(value);
  // the stored value changed elsewhere (undo, another view): show it
  if (!Object.is(value, shown)) {
    setShown(value);
    if (Number(text) !== value || text.trim() === "") setText(String(value));
  }
  const invalid = text.trim() === "" || !Number.isFinite(Number(text));
  return (
    <input
      type="number"
      className="ss-input w-full"
      value={text}
      step="any"
      aria-invalid={invalid || undefined}
      title={invalid ? `Enter a number (leaving the field keeps ${value})` : undefined}
      onChange={(e) => {
        setText(e.target.value);
        const t = e.target.value.trim();
        if (t !== "" && Number.isFinite(Number(t))) onChange(Number(t));
      }}
      onBlur={() => invalid && setText(String(value))}
    />
  );
}

function ParameterInput({
  def,
  value,
  onChange,
}: {
  def: ParameterDef;
  value: ParamValue;
  onChange: (v: ParamValue) => void;
}) {
  switch (def.type) {
    case "boolean":
      return (
        <input
          type="checkbox"
          checked={Boolean(value)}
          onChange={(e) => onChange(e.target.checked)}
        />
      );
    case "enum":
      return (
        <select
          className="ss-input w-full"
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
        >
          {(def.options ?? []).map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      );
    case "number":
      return <NumberInput value={Number(value)} onChange={onChange} />;
    case "code":
      return (
        <textarea
          className="ss-input w-full resize-y font-mono text-[11px]"
          rows={12}
          spellCheck={false}
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
        />
      );
    default:
      return def.key === "profile" ? (
        <textarea
          className="ss-input w-full resize-y font-mono text-[11px]"
          rows={2}
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
        />
      ) : (
        <input
          className="ss-input w-full"
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
        />
      );
  }
}

function slugify(name: string): string {
  return name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "port";
}

/** Add/rename/remove per-instance signal ports (Script, Monitor). */
function DynamicPortsEditor({ element }: { element: ElementInstance }) {
  const setDynamicPorts = useProjectStore((s) => s.setDynamicPorts);
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
      <div className="mb-1 flex items-center gap-2 text-[11px] font-semibold text-[color:var(--ss-text-dim)]">
        Signal Ports
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[10px] font-normal"
          onClick={() => add("input")}
        >
          <Plus size={10} /> input
        </button>
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[10px] font-normal"
          onClick={() => add("output")}
        >
          <Plus size={10} /> output
        </button>
      </div>
      {ports.length === 0 && (
        <div className="px-1 pb-1 text-[11px] italic text-[color:var(--ss-text-dim)]">
          No ports yet — add inputs/outputs; their names are the keys in the
          script's <code>inputs</code> / returned dict.
        </div>
      )}
      {ports.map((p) => (
        <div key={p.id} className="flex items-center gap-1 py-0.5">
          <span
            className={`rounded px-1 text-[9px] font-semibold uppercase ${
              p.direction === "input" ? "bg-[#e0f2f7] text-[#0e7490]" : "bg-[#fdf1e2] text-[#b45309]"
            }`}
          >
            {p.direction === "input" ? "in" : "out"}
          </span>
          <input
            className="ss-input flex-1 font-mono text-[11px]"
            defaultValue={p.name}
            title="Port name (renaming disconnects existing wires)"
            onBlur={(e) => e.target.value !== p.name && rename(p, e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
          <button
            className="text-[color:var(--ss-text-dim)] hover:text-red-600"
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

export function ElementForm({
  element,
  def,
  compact = false,
}: {
  element: ElementInstance;
  def: ComponentDef;
  /** In the narrow Properties panel, table/code/profile params collapse to an
   *  "Edit…" button that opens the roomier parameter dialog. */
  compact?: boolean;
}) {
  const setParameter = useProjectStore((s) => s.setParameter);
  const renameElement = useProjectStore((s) => s.renameElement);
  const setActiveSystem = useProjectStore((s) => s.setActiveSystem);
  const running = useProjectStore((s) => s.running);
  const openParamDialog = useUIStore((s) => s.openParamDialog);

  // profiles are stored as strings but edited as a full-width grid, so they
  // join the tables/code in the "big" (full-width) group rather than the
  // compact scalar table.
  const isProfile = (p: ParameterDef) => p.type === "string" && p.key === "profile";
  const isBig = (p: ParameterDef) =>
    p.type === "table1d" || p.type === "table2d" || p.type === "code" || isProfile(p);
  const scalarParams = useMemo(() => def.parameters.filter((p) => !isBig(p)), [def]);
  const bigParams = useMemo(() => def.parameters.filter(isBig), [def]);
  const valueOf = (p: ParameterDef): ParamValue =>
    element.parameterOverrides[p.key] ?? p.default;

  return (
    <div className="flex flex-col gap-2 p-2">
      <div className="flex items-center gap-2">
        <span className="w-[72px] shrink-0 text-[11px] text-[color:var(--ss-text-dim)]">Name</span>
        <input
          className="ss-input flex-1"
          value={element.label}
          onChange={(e) => renameElement(element.id, e.target.value)}
        />
      </div>
      <div className="flex items-center gap-2 text-[11px] text-[color:var(--ss-text-dim)]">
        <span className="w-[72px] shrink-0">Type</span>
        <span className="text-[color:var(--ss-text)]">{def.name}</span>
        {running && (
          <span
            className="flex items-center gap-1 rounded bg-[#e5f5eb] px-1.5 py-0.5 text-[10px] font-semibold text-emerald-700"
            title="Simulation running — number/switch edits apply immediately; tables and code apply on the next run"
          >
            <Radio size={10} /> LIVE
          </span>
        )}
        <span className="ml-auto rounded bg-[color:var(--ss-panel-alt)] px-1.5 py-0.5 text-[10px] uppercase">
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
      {scalarParams.length > 0 && (
        <table className="w-full border-collapse">
          <thead>
            <tr>
              <th className="ss-th">Parameter</th>
              <th className="ss-th w-[110px]">Value</th>
              <th className="ss-th w-[52px]">Unit</th>
            </tr>
          </thead>
          <tbody>
            {scalarParams.map((p) => (
              <tr key={p.key}>
                <td className="ss-td text-[11px]">
                  {p.label}
                  {running && p.variability === "fixed" && (
                    <span
                      className="ml-1 text-[10px] italic text-[color:var(--ss-text-dim)]"
                      title="Structural parameter — a live edit takes effect on the next run"
                    >
                      (next run)
                    </span>
                  )}
                </td>
                <td className="ss-td">
                  <ParameterInput
                    def={p}
                    value={valueOf(p)}
                    onChange={(v) => setParameter(element.id, p.key, v)}
                  />
                </td>
                <td className="ss-td text-[11px] text-[color:var(--ss-text-dim)]">{p.unit}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {compact && bigParams.length > 0 && (
        <div className="flex flex-col gap-1">
          {bigParams.map((p) => (
            <button
              key={p.key}
              className="ss-toolbtn justify-between border border-[color:var(--ss-border)] px-2 py-1"
              onClick={() => openParamDialog(element.id)}
              title="Open the full editor in a dialog"
            >
              <span className="flex items-center gap-1.5">
                {p.type === "code" ? <Pencil size={12} /> : <Table2 size={12} />}
                {isProfile(p) ? "Profile" : p.label}
                {p.type !== "code" && !isProfile(p) ? (
                  <span className="text-[10px] text-[color:var(--ss-text-dim)]">({p.unit})</span>
                ) : null}
              </span>
              <span className="text-[10px] text-[color:var(--ss-accent)]">Edit…</span>
            </button>
          ))}
        </div>
      )}
      {!compact &&
        bigParams.map((p) => (
        <div key={p.key}>
          <div className="mb-1 text-[11px] font-semibold text-[color:var(--ss-text-dim)]">
            {isProfile(p) ? "Profile" : p.label}
            {!isProfile(p) && p.type !== "code" ? ` (${p.unit})` : ""}
            {running && !isProfile(p) && (
              <span className="ml-1 font-normal italic">— applies on next run</span>
            )}
          </div>
          {isProfile(p) ? (
            <ProfileGridEditor
              value={String(valueOf(p))}
              defId={def.id}
              mode={String(element.parameterOverrides.mode ?? def.parameters.find((q) => q.key === "mode")?.default ?? "")}
              onChange={(v) => setParameter(element.id, p.key, v)}
            />
          ) : p.type === "table1d" && p.axes?.length === 1 ? (
            <Table1DEditor
              value={valueOf(p) as Table1D}
              axis={p.axes[0]}
              valueLabel={p.label}
              valueUnit={p.unit}
              onChange={(v) => setParameter(element.id, p.key, v)}
            />
          ) : p.type === "table2d" && p.axes?.length === 2 ? (
            <Table2DEditor
              value={valueOf(p) as Table2D}
              axes={[p.axes[0], p.axes[1]]}
              valueLabel={p.label}
              valueUnit={p.unit}
              onChange={(v) => setParameter(element.id, p.key, v)}
            />
          ) : (
            <ParameterInput
              def={p}
              value={valueOf(p)}
              onChange={(v) => setParameter(element.id, p.key, v)}
            />
          )}
        </div>
      ))}
      {def.allowDynamicPorts && <DynamicPortsEditor element={element} />}
      {def.ports.length > 0 && (
        <div>
          <div className="mb-1 text-[11px] font-semibold text-[color:var(--ss-text-dim)]">
            Ports
          </div>
          {def.ports.map((p) => (
            <div key={p.id} className="flex items-center gap-2 py-0.5 text-[11px]">
              <span
                className="h-2 w-2 shrink-0 rounded-full"
                style={{
                  background:
                    p.kind === "electrical"
                      ? "#e08600"
                      : p.kind === "mechanical"
                        ? "#3f4650"
                        : p.kind === "signal"
                          ? "#0e7490"
                          : "#c2410c",
                }}
              />
              <span className="truncate">{p.name}</span>
              <span className="ml-auto text-[10px] text-[color:var(--ss-text-dim)]">
                {p.kind} · {p.direction}
              </span>
            </div>
          ))}
        </div>
      )}
      {def.description && (
        <p className="border-t border-[color:var(--ss-border)] pt-2 text-[11px] italic text-[color:var(--ss-text-dim)]">
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
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar text-[11px] font-semibold">
        {selected ? `Parameters — ${selected.label}` : "Parameters"}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {selected && selectedDef ? (
          <ElementForm element={selected} def={selectedDef} compact />
        ) : (
          <div className="flex flex-col items-center gap-2 p-6 text-center text-[12px] text-[color:var(--ss-text-dim)]">
            <Box size={22} strokeWidth={1.2} />
            Select an element on the canvas or in the Elements panel to edit its parameters.
          </div>
        )}
      </div>
    </div>
  );
}
