import { useEffect, useId, useRef, useState } from "react";
import { FileUp, X } from "lucide-react";
import * as api from "../api";
import type { Table1D, Table2D } from "../types";

export interface ImportTarget {
  componentDefId: string;
  paramKey: string;
  /** the Road Profile's mode: its x axis is distance or time */
  mode?: string;
}

const UNIT_LABELS: Record<string, string> = {
  x: "the first column",
  y: "the second column",
  cols: "the column values",
  rows: "the row values",
  value: "the map's values",
};

/** "Import from file…" next to a table, map or profile grid (STD-10): pick a
 *  CSV or Excel file, check what was read in a preview, then apply it as one
 *  undo step. The engine finds the data and converts the units. */
export function ImportFromFileButton({
  target,
  onApply,
}: {
  target: ImportTarget;
  onApply: (value: Table1D | Table2D | string) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [file, setFile] = useState<{ name: string; data: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <button
        className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px]"
        title="Read this table from a CSV or Excel (.xlsx) file; you see what was read before anything changes"
        onClick={() => input.current?.click()}
      >
        <FileUp size={12} /> Import from file…
      </button>
      <input
        ref={input}
        type="file"
        accept=".csv,.tsv,.txt,.xlsx,.xlsm,text/csv,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        className="hidden"
        aria-label="Table file to import"
        onChange={async (e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          if (!f) return;
          try {
            setError(null);
            setFile({ name: f.name, data: await api.fileToBase64(f) });
          } catch (err) {
            setError(`The file could not be read: ${(err as Error).message}`);
          }
        }}
      />
      {error && (
        <span role="alert" className="ss-param-problem text-[11px]">
          {error}
        </span>
      )}
      {file && (
        <ImportTableDialog
          file={file}
          target={target}
          onClose={() => setFile(null)}
          onApply={(v) => {
            onApply(v);
            setFile(null);
          }}
        />
      )}
    </>
  );
}

function ImportTableDialog({
  file,
  target,
  onClose,
  onApply,
}: {
  file: { name: string; data: string };
  target: ImportTarget;
  onClose: () => void;
  onApply: (value: Table1D | Table2D | string) => void;
}) {
  const [options, setOptions] = useState<api.TableImportOptions>({});
  const [result, setResult] = useState<api.TableImport | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [rangeText, setRangeText] = useState("");
  const titleId = useId();
  const applyRef = useRef<HTMLButtonElement>(null);

  // the target is rebuilt on every render of the editor: compare its content
  const targetKey = JSON.stringify(target);
  useEffect(() => {
    let stale = false;
    api
      .importTableFile(file, JSON.parse(targetKey) as ImportTarget, options)
      .then((r) => {
        if (stale) return;
        setResult(r);
        setFailure(null);
      })
      .catch((e: Error) => !stale && setFailure(e.message.replace(/^\d+ /, "")));
    return () => {
      stale = true;
    };
  }, [file, targetKey, options]);

  useEffect(() => {
    if (result?.ok) applyRef.current?.focus();
  }, [result?.ok]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const set = (patch: api.TableImportOptions) => setOptions((o) => ({ ...o, ...patch }));
  const colName = (c: api.TableImport["columns"][number]) =>
    `${c.letter}: ${c.header || "(no header)"}`;

  return (
    <div
      className="ss-import-dialog fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="flex max-h-[86vh] w-[min(620px,calc(100vw-32px))] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="flex items-center gap-2 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-3 py-2">
          <FileUp size={15} className="text-[color:var(--ss-accent)]" />
          <span id={titleId} className="min-w-0 truncate text-[13px] font-semibold">
            Import {result?.target ?? "table"} from {file.name}
          </span>
          <button className="ss-toolbtn ml-auto" title="Close (Esc)" aria-label="Close the dialog" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
        <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3 text-[12px]">
          {failure && (
            <p role="alert" className="ss-param-problem">
              {failure}
            </p>
          )}
          {!result && !failure && <p className="text-[color:var(--ss-text-dim)]">Reading the file…</p>}
          {result && (
            <>
              <div className="flex flex-wrap items-end gap-2">
                {result.sheets.length > 1 && (
                  <label className="flex flex-col gap-0.5">
                    <span className="text-[11px] text-[color:var(--ss-text-dim)]">Sheet</span>
                    <select
                      className="ss-input"
                      value={result.sheet}
                      onChange={(e) => setOptions({ sheet: e.target.value })}
                    >
                      {result.sheets.map((s) => (
                        <option key={s.name} value={s.name}>
                          {s.name}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                <label className="flex flex-col gap-0.5">
                  <span className="text-[11px] text-[color:var(--ss-text-dim)]">Cells (e.g. B3:F20)</span>
                  <input
                    className="ss-input w-[110px]"
                    value={rangeText}
                    placeholder={result.range || "whole sheet"}
                    onChange={(e) => setRangeText(e.target.value)}
                    onBlur={() => set({ range: rangeText.trim() || undefined })}
                    onKeyDown={(e) => e.key === "Enter" && set({ range: rangeText.trim() || undefined })}
                  />
                </label>
                {result.kind !== "table2d" && result.columns.length > 2 && (
                  <>
                    <label className="flex flex-col gap-0.5">
                      <span className="text-[11px] text-[color:var(--ss-text-dim)]">
                        {result.axes[0]?.name} from
                      </span>
                      <select
                        className="ss-input"
                        value={result.xColumn ?? ""}
                        onChange={(e) => set({ xColumn: Number(e.target.value), units: undefined })}
                      >
                        {result.columns.map((c) => (
                          <option key={c.index} value={c.index}>
                            {colName(c)}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="flex flex-col gap-0.5">
                      <span className="text-[11px] text-[color:var(--ss-text-dim)]">{result.valueName} from</span>
                      <select
                        className="ss-input"
                        value={result.yColumn ?? ""}
                        onChange={(e) => set({ yColumn: Number(e.target.value), units: undefined })}
                      >
                        {result.columns.map((c) => (
                          <option key={c.index} value={c.index}>
                            {colName(c)}
                          </option>
                        ))}
                      </select>
                    </label>
                  </>
                )}
                {result.kind === "table2d" && (
                  <label className="flex items-center gap-1 pb-1">
                    <input
                      type="checkbox"
                      checked={result.transpose}
                      onChange={(e) => set({ transpose: e.target.checked, units: undefined })}
                    />
                    Swap rows and columns
                  </label>
                )}
              </div>

              {Object.keys(result.units).length > 0 && (
                <div className="flex flex-wrap gap-2">
                  {Object.entries(result.units).map(([k, u]) => (
                    <label key={k} className="flex flex-col gap-0.5">
                      <span className="text-[11px] text-[color:var(--ss-text-dim)]">
                        Unit of {UNIT_LABELS[k] ?? k}
                        {u.how === "header" ? " (from the file)" : u.how === "guessed" ? " (guessed)" : ""}
                      </span>
                      <select
                        className="ss-input"
                        value={u.used}
                        onChange={(e) => set({ units: { ...(options.units ?? {}), [k]: e.target.value } })}
                      >
                        {u.options.map((o) => (
                          <option key={o} value={o}>
                            {o === u.target ? `${o} (as stored)` : o === "fraction" ? "fraction (0.05 = 5 %)" : o}
                          </option>
                        ))}
                      </select>
                    </label>
                  ))}
                </div>
              )}
              {Object.values(result.units)
                .filter((u) => u.question)
                .map((u) => (
                  <p key={u.question} className="rounded border border-[color:var(--ss-warning,#b58900)] px-2 py-1">
                    {u.question} Change the unit above if not.
                  </p>
                ))}

              {result.errors.length > 0 && (
                <div role="alert">
                  <p className="font-semibold">The file cannot be imported as it is:</p>
                  <ul className="ml-4 list-disc">
                    {result.errors.map((e, i) => (
                      <li key={i} className="ss-param-problem">
                        {e.text}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {result.warnings.length > 0 && (
                <ul className="ml-4 list-disc text-[color:var(--ss-text-dim)]">
                  {result.warnings.slice(0, 5).map((w, i) => (
                    <li key={i}>{w.text}</li>
                  ))}
                </ul>
              )}
              {result.ok && result.preview && <Preview result={result} />}
              {result.notes.length > 0 && (
                <p className="text-[11px] text-[color:var(--ss-text-dim)]">{result.notes.join(" · ")}</p>
              )}
              {!result.ok && result.cells.length > 0 && <Cells cells={result.cells} />}
            </>
          )}
        </div>
        <div className="flex items-center justify-end gap-2 border-t border-[color:var(--ss-border)] px-3 py-2">
          <span className="mr-auto text-[11px] text-[color:var(--ss-text-dim)]">
            {result?.ok ? "Replaces the whole table; Undo (Ctrl+Z) brings it back." : ""}
          </span>
          <button className="ss-toolbtn border border-[color:var(--ss-border)] px-2" onClick={onClose}>
            Cancel
          </button>
          <button
            ref={applyRef}
            className="ss-toolbtn border border-[color:var(--ss-accent)] bg-[color:var(--ss-accent)] px-2 text-white disabled:opacity-40"
            disabled={!result?.ok || result.value == null}
            onClick={() => result?.value != null && onApply(result.value)}
          >
            Apply{result?.ok ? ` (${result.points.toLocaleString("en")} points)` : ""}
          </button>
        </div>
      </div>
    </div>
  );
}

const fmt = (v: number) => (Math.abs(v) >= 1000 ? v.toLocaleString("en", { maximumFractionDigits: 1 }) : String(+v.toPrecision(4)));

/** What will be stored: a 1-D table or profile as a small line chart, a map
 *  as its first rows and columns. */
function Preview({ result }: { result: api.TableImport }) {
  const p = result.preview;
  if (!p) return null;
  if (Array.isArray(p)) {
    const xs = p.map((q) => q[0]);
    const ys = p.map((q) => q[1]);
    const [x0, x1] = [Math.min(...xs), Math.max(...xs)];
    const [y0, y1] = [Math.min(...ys), Math.max(...ys)];
    const W = 560;
    const H = 150;
    const sx = (x: number) => 40 + ((x - x0) / (x1 - x0 || 1)) * (W - 50);
    const sy = (y: number) => H - 20 - ((y - y0) / (y1 - y0 || 1)) * (H - 30);
    const axis = result.axes[0];
    return (
      <figure className="m-0">
        <svg
          viewBox={`0 0 ${W} ${H}`}
          className="w-full rounded border border-[color:var(--ss-border)]"
          role="img"
          aria-label={`Preview: ${result.valueName} from ${fmt(y0)} to ${fmt(y1)} ${result.valueUnit} over ${axis.name} ${fmt(x0)} to ${fmt(x1)} ${axis.unit}`}
        >
          <polyline
            fill="none"
            stroke="var(--ss-accent)"
            strokeWidth={1.5}
            points={p.map(([x, y]) => `${sx(x).toFixed(1)},${sy(y).toFixed(1)}`).join(" ")}
          />
          <text x={4} y={14} fontSize={10} fill="var(--ss-text-dim)">
            {fmt(y1)}
          </text>
          <text x={4} y={H - 22} fontSize={10} fill="var(--ss-text-dim)">
            {fmt(y0)}
          </text>
          <text x={40} y={H - 5} fontSize={10} fill="var(--ss-text-dim)">
            {fmt(x0)}
          </text>
          <text x={W - 6} y={H - 5} fontSize={10} textAnchor="end" fill="var(--ss-text-dim)">
            {fmt(x1)} {axis.unit}
          </text>
        </svg>
        <figcaption className="text-[11px] text-[color:var(--ss-text-dim)]">
          {result.valueName} ({result.valueUnit}) over {axis.name} ({axis.unit}), {p.length.toLocaleString("en")} points, as it will be stored
        </figcaption>
      </figure>
    );
  }
  const [outer, inner] = result.axes;
  const cols = p.cols.slice(0, 8);
  const rows = p.rows.slice(0, 8);
  return (
    <figure className="m-0 overflow-x-auto">
      <table className="border-collapse text-[11px]">
        <thead>
          <tr>
            <th className="border border-[color:var(--ss-border)] px-1 text-left font-normal text-[color:var(--ss-text-dim)]">
              {inner.name} \ {outer.name}
            </th>
            {cols.map((c) => (
              <th key={c} className="border border-[color:var(--ss-border)] px-1">
                {fmt(c)}
              </th>
            ))}
            {p.cols.length > cols.length && <th className="px-1">…</th>}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={r}>
              <th className="border border-[color:var(--ss-border)] px-1">{fmt(r)}</th>
              {cols.map((_, j) => (
                <td key={j} className="border border-[color:var(--ss-border)] px-1 text-right">
                  {fmt(p.values[j][i])}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <figcaption className="text-[11px] text-[color:var(--ss-text-dim)]">
        Columns: {outer.name} ({outer.unit}), {p.cols.length}; rows: {inner.name} ({inner.unit}), {p.rows.length};
        values: {result.valueName} ({result.valueUnit}), as they will be stored
      </figcaption>
    </figure>
  );
}

/** The first cells of the sheet as read, to see where the data is. */
function Cells({ cells }: { cells: api.TableImport["cells"] }) {
  const width = Math.min(10, Math.max(...cells.map((r) => r.length)));
  const letter = (i: number) => String.fromCharCode(65 + i);
  return (
    <div className="overflow-x-auto">
      <p className="text-[11px] text-[color:var(--ss-text-dim)]">The start of the sheet:</p>
      <table className="border-collapse text-[11px]">
        <thead>
          <tr>
            <th />
            {Array.from({ length: width }, (_, i) => (
              <th key={i} className="px-1 font-normal text-[color:var(--ss-text-dim)]">
                {letter(i)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {cells.slice(0, 12).map((r, i) => (
            <tr key={i}>
              <th className="pr-1 font-normal text-[color:var(--ss-text-dim)]">{i + 1}</th>
              {Array.from({ length: width }, (_, j) => (
                <td key={j} className="max-w-[90px] truncate border border-[color:var(--ss-border)] px-1">
                  {r[j] ?? ""}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
