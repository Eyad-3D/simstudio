// Import table data in any layout, from a file or pasted text: one importer
// (backend/app/dataio/layouts.py) and one preview. The preview shows the
// layout found, what each column, row or label was read as, with its unit
// and conversion, every problem with its cell, and the resulting table in the
// editor's own grid with its plot. The user corrects the matching and sees
// the result change; nothing is written before Apply, which is one undo step.
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ClipboardPaste, FileUp, X } from "lucide-react";
import * as api from "../api";
import { useOwnerWindow } from "../ownerWindow";
import { useProjectStore } from "../store/projectStore";
import type { Grid } from "../tableN";
import type { TableN } from "../types";
import { Cells } from "./ImportTableDialog";
import { TableNEditor, type SetGridUnit, type ShownQuantity } from "./panels/TableNEditor";

/** Where the data comes from: a file, or text pasted into a table (and why
 *  the preview opened instead of pasting it into the cells). */
export type ImportSource = { file: { name: string; data: string } } | { text: string; why: string };

/** What Apply stores: the tables by value ("value" for a library table, an
 *  output's id for a Map block); for a Map block set from the data, its ports. */
export interface DataApply {
  tables: Record<string, TableN>;
  ports: api.DataPort[] | null;
}

const ACCEPT =
  ".csv,.tsv,.txt,.xlsx,.xlsm,text/csv,text/plain,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/** The import of a table: its "Import…" button (with the file chooser), a
 *  handler for pasted text the grid cannot take as it is, and the dialog. */
export function useTableImport(target: api.DataTargetRef | null, onApply: (r: DataApply) => void) {
  const [source, setSource] = useState<ImportSource | null>(null);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  if (!target) return { tools: null as ReactNode, onImportText: undefined, dialog: null as ReactNode };
  const tools = (
    <>
      <button
        type="button"
        className="ss-toolbtn"
        title="Read this table from an Excel (.xlsx), CSV or text file, in any layout: you see what was read, and how, before anything changes"
        onClick={() => input.current?.click()}
      >
        <FileUp size={12} aria-hidden /> Import…
      </button>
      <input
        ref={input}
        type="file"
        accept={ACCEPT}
        className="hidden"
        aria-label="Table file to import"
        onChange={async (e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          if (!f) return;
          try {
            setError(null);
            setSource({ file: { name: f.name, data: await api.fileToBase64(f) } });
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
    </>
  );
  const dialog = source && (
    <ImportDataDialog
      source={source}
      target={target}
      onClose={() => setSource(null)}
      onApply={(r) => {
        onApply(r);
        setSource(null);
      }}
    />
  );
  return { tools, onImportText: (text: string, why: string) => setSource({ text, why }), dialog };
}

const fieldLabel = (f: api.DataField) => f.title || f.where;

/** The conversion a field's numbers go through, in words. */
function conversion(f: api.DataField): string {
  if (f.slot === "skip" || !f.stored || f.bad) return "";
  if (f.assumed) return `no unit in the data: read in ${f.stored}`;
  const shown = f.written ?? f.used;
  if (f.used && f.used !== f.stored) return `${shown} → ${f.stored}`;
  if (shown && shown !== f.stored) return `${shown} = ${f.stored}`;
  return "";
}

export function ImportDataDialog({
  source,
  target,
  onClose,
  onApply,
}: {
  source: ImportSource;
  target: api.DataTargetRef;
  onClose: () => void;
  onApply: (r: DataApply) => void;
}) {
  const [options, setOptions] = useState<api.DataImportOptions>({});
  const [result, setResult] = useState<api.DataImport | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // the preview's own grid can be edited, and is what Apply stores
  const [edited, setEdited] = useState<Record<string, Grid> | null>(null);
  const [shownUnits, setShownUnits] = useState<Record<string, string>>({});
  const unitGroups = useProjectStore((s) => s.unitGroups);
  const titleId = useId();
  const applyRef = useRef<HTMLButtonElement>(null);
  const targetKey = JSON.stringify(target);

  useEffect(() => {
    let stale = false;
    const from = "file" in source ? { file: source.file } : { text: source.text };
    api
      .importData(from, JSON.parse(targetKey) as api.DataTargetRef, options)
      .then((r) => {
        if (stale) return;
        setResult(r);
        setEdited(null);
        setFailure(null);
      })
      .catch((e: Error) => !stale && setFailure(e.message.replace(/^\d+ /, "")));
    return () => {
      stale = true;
    };
  }, [source, targetKey, options]);

  useEffect(() => {
    if (result?.ok) applyRef.current?.focus();
  }, [result?.ok]);

  const win = useOwnerWindow();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    win.addEventListener("keydown", onKey);
    return () => win.removeEventListener("keydown", onKey);
  }, [onClose, win]);

  const set = (patch: api.DataImportOptions) => setOptions((o) => ({ ...o, ...patch }));
  const setField = (i: number, slot: string) => set({ fields: { ...options.fields, [i]: slot }, build: options.build });
  const setFieldUnit = (i: number, unit: string) => set({ units: { ...options.units, [i]: unit } });

  const build = Boolean(result?.build);
  const slotChoices = (f: api.DataField) => {
    const out: { value: string; text: string }[] = [];
    if (build) {
      if (f.role === "axis" || !f.fixed) out.push({ value: "axis:+", text: "an input (an axis)" });
      if (f.role === "value" || !f.fixed) out.push({ value: "value:+", text: "an output (a table)" });
    } else if (result) {
      if (f.role === "axis" || !f.fixed)
        result.targetAxes.forEach((a, i) => out.push({ value: `axis:${i}`, text: `the ${a.name} axis` }));
      if (f.role === "value" || !f.fixed)
        result.targetValues.forEach((v, i) => out.push({ value: `value:${i}`, text: `${v.name} values` }));
    }
    out.push({ value: "skip", text: "not used" });
    return out;
  };
  const slotValue = (f: api.DataField) => (build && f.slot !== "skip" ? `${f.slot.split(":")[0]}:+` : f.slot);

  // the preview grid: the table as it will be stored, in the editor's own grid
  const quantities = (): { axes: ShownQuantity[]; outputs: (ShownQuantity & { key: string })[] } | null => {
    if (!result?.ok || !result.tables) return null;
    const shown = (key: string, lib: string) => ({ lib, unit: shownUnits[key] ?? lib });
    if (build && result.ports) {
      const of = (p: api.DataPort) => {
        const lib = p.unitText ?? unitGroups[p.unitGroup] ?? "-";
        return p.unitText ? { lib, unit: lib, fixed: true } : { lib, unit: shownUnits[p.id] ?? p.display ?? lib };
      };
      const ins = result.ports.filter((p) => p.direction === "input");
      return {
        axes: ins.map((p) => ({ name: p.name, ...of(p) })),
        outputs: result.ports.filter((p) => p.direction === "output").map((p) => ({ key: p.id, name: p.name, ...of(p) })),
      };
    }
    return {
      axes: result.targetAxes.map((a, i) => ({ name: a.name, ...shown(`axis:${i}`, a.unit) })),
      outputs: result.targetValues.map((v) => ({ key: v.key, name: v.name, ...shown(`value:${v.key}`, v.unit) })),
    };
  };
  const q = quantities();
  const tables = (edited ?? result?.tables ?? {}) as Record<string, Grid>;
  const setUnit: SetGridUnit = (slot, output, unit, again) => {
    const key = build && result?.ports
      ? slot === "value"
        ? output
        : result.ports.filter((p) => p.direction === "input")[slot].id
      : slot === "value"
        ? `value:${output}`
        : `axis:${slot}`;
    setShownUnits((s) => ({ ...s, [key]: unit }));
    if (again) setEdited({ ...tables, ...again });
  };

  const problems = result?.problems ?? [];
  const errors = problems.filter((p) => p.level === "error");
  const questions = problems.filter((p) => p.level === "question");
  const warnings = problems.filter((p) => p.level === "warning");
  const marked = new Set(errors.map((p) => p.cell).filter(Boolean) as string[]);
  const from = "file" in source ? source.file.name : "the clipboard";
  const inputs = result?.ports?.filter((p) => p.direction === "input").length ?? 0;

  return (
    <div
      className="ss-import-dialog fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-import-preview
        className="flex max-h-[90vh] w-[min(760px,calc(100vw-32px))] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="flex items-center gap-2 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-3 py-2">
          {"file" in source ? (
            <FileUp size={15} className="text-[color:var(--ss-accent)]" />
          ) : (
            <ClipboardPaste size={15} className="text-[color:var(--ss-accent)]" />
          )}
          <span id={titleId} className="min-w-0 truncate text-[13px] font-semibold">
            Import {result?.target ?? "a table"} from {from}
          </span>
          <button className="ss-toolbtn ml-auto" title="Close (Esc)" aria-label="Close the dialog" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
        <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3 text-[12px]">
          {"why" in source && (
            <p className="text-[color:var(--ss-text-dim)]" data-import-why>
              Not pasted into the cells: {source.why} Check how it was read, then Apply.
            </p>
          )}
          {failure && (
            <p role="alert" className="ss-param-problem">
              {failure}
            </p>
          )}
          {!result && !failure && <p className="text-[color:var(--ss-text-dim)]">Reading…</p>}
          {result && (
            <>
              <p data-import-layout>
                <span className="font-semibold">Found: </span>
                {result.layout || "no table"}
                {result.read.length > 0 && (
                  <span className="text-[color:var(--ss-text-dim)]"> · read as {result.read.join("; ")}</span>
                )}
              </p>
              <div className="flex flex-wrap items-end gap-2">
                {(result.sheets?.length ?? 0) > 1 && (
                  <label className="flex flex-col gap-0.5">
                    <span className="text-[11px] text-[color:var(--ss-text-dim)]">Sheet</span>
                    <select
                      className="ss-input"
                      value={result.sheet ?? ""}
                      onChange={(e) => setOptions({ sheet: e.target.value })}
                    >
                      <option value="*">Every sheet (one per breakpoint)</option>
                      {result.sheets!.map((s) => (
                        <option key={s.name} value={s.name}>
                          {s.name}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                {result.block !== null && (
                  <label className="flex flex-col gap-0.5">
                    <span className="text-[11px] text-[color:var(--ss-text-dim)]">Table</span>
                    <select
                      className="ss-input"
                      value={result.block}
                      onChange={(e) => set({ block: Number(e.target.value) })}
                    >
                      {result.blocks.map((b, i) => (
                        <option key={i} value={i}>
                          {b.range}
                          {b.label ? ` (${b.label})` : ""}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                {result.decimal && (result.decimalQuestion || options.decimal) && (
                  <label className="flex flex-col gap-0.5">
                    <span className="text-[11px] text-[color:var(--ss-text-dim)]">Decimal mark</span>
                    <select
                      className="ss-input"
                      value={result.decimal}
                      onChange={(e) => set({ decimal: e.target.value as "comma" | "point" })}
                    >
                      <option value="comma">comma (1,5 = 1.5; 1.000 = 1000)</option>
                      <option value="point">point (1.5; 1,000 = 1000)</option>
                    </select>
                  </label>
                )}
              </div>
              {(result.canBuild || result.build) && (
                <label className="flex items-start gap-1.5 rounded border border-[color:var(--ss-border)] px-2 py-1">
                  <input
                    type="checkbox"
                    checked={build}
                    onChange={(e) => setOptions((o) => ({ ...o, build: e.target.checked, fields: undefined, units: undefined }))}
                  />
                  <span>
                    Set the Map block&apos;s inputs and outputs from the data
                    {build && result.ports
                      ? ` (${inputs} inputs, ${result.ports.length - inputs} outputs, named and with units from the titles)`
                      : ""}
                    <span className="block text-[11px] text-[color:var(--ss-text-dim)]">
                      Its inputs and outputs are replaced; wires to a port whose name is not in the data are removed.
                    </span>
                  </span>
                </label>
              )}
              {result.fields.length > 0 && (
                <table className="ss-import-fields w-full border-collapse text-[11px]" aria-label="What was read">
                  <thead className="text-left text-[color:var(--ss-text-dim)]">
                    <tr>
                      <th className="font-normal">In the data</th>
                      <th className="font-normal">Is</th>
                      <th className="font-normal">Its unit</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.fields.map((f) => (
                      <tr key={f.index} className="border-t border-[color:var(--ss-border)]">
                        <td className="py-0.5 pr-2">
                          {f.title || <i>no title</i>}
                          <span className="text-[color:var(--ss-text-dim)]">
                            {" "}
                            · {f.where}
                            {f.count ? `, ${f.count} breakpoint${f.count === 1 ? "" : "s"}` : ""}
                          </span>
                        </td>
                        <td className="py-0.5 pr-2">
                          <select
                            className="ss-input py-0.5 text-[11px]"
                            aria-label={`What ${fieldLabel(f)} is`}
                            value={slotValue(f)}
                            onChange={(e) => setField(f.index, e.target.value)}
                          >
                            {slotChoices(f).map((o) => (
                              <option key={o.value} value={o.value}>
                                {o.text}
                              </option>
                            ))}
                          </select>
                          {f.how && !["chosen", "build", "only"].includes(f.how) && f.slot !== "skip" && (
                            <span className="text-[color:var(--ss-text-dim)]">
                              {f.how === "order" ? " (by its place: check it)" : ` (by its ${f.how})`}
                            </span>
                          )}
                        </td>
                        <td className="py-0.5">
                          {f.slot !== "skip" && f.options && f.options.length > 1 ? (
                            <select
                              className="ss-input py-0.5 text-[11px]"
                              aria-label={`Unit of ${fieldLabel(f)}`}
                              value={f.used ?? ""}
                              onChange={(e) => setFieldUnit(f.index, e.target.value)}
                            >
                              {[...new Set([...(f.used ? [f.used] : []), ...f.options])].map((u) => (
                                <option key={u} value={u}>
                                  {u}
                                </option>
                              ))}
                            </select>
                          ) : (
                            <span>{f.used ?? f.written ?? ""}</span>
                          )}
                          <span className="text-[color:var(--ss-text-dim)]"> {conversion(f)}</span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              {questions.map((p, i) => (
                <p key={`q${i}`} className="rounded border border-[color:var(--ss-warn)] px-2 py-1">
                  {p.text}
                </p>
              ))}
              {result.decimalQuestion && (
                <p className="rounded border border-[color:var(--ss-warn)] px-2 py-1">
                  {result.decimalQuestion} Change the decimal mark above if that is wrong.
                </p>
              )}
              {(result.holes > 0 || options.fill) && (
                <fieldset className="flex flex-wrap gap-3 rounded border border-[color:var(--ss-border)] px-2 py-1">
                  <legend className="px-1 text-[11px] text-[color:var(--ss-text-dim)]">
                    {result.holes.toLocaleString("en")} point{result.holes === 1 ? "" : "s"} of the grid missing
                  </legend>
                  <label className="flex items-center gap-1">
                    <input
                      type="radio"
                      name="fill"
                      checked={(options.fill ?? "refuse") === "refuse"}
                      onChange={() => set({ fill: "refuse" })}
                    />
                    Do not import: fill the data in first
                  </label>
                  <label className="flex items-center gap-1">
                    <input
                      type="radio"
                      name="fill"
                      checked={options.fill === "nearest"}
                      onChange={() => set({ fill: "nearest" })}
                    />
                    Fill each from the nearest breakpoint with a value
                  </label>
                </fieldset>
              )}
              {errors.length > 0 && (
                <div role="alert">
                  <p className="font-semibold">Not imported yet:</p>
                  <ul className="ml-4 list-disc">
                    {errors.map((e, i) => (
                      <li key={i} className="ss-param-problem" data-cell={e.cell}>
                        {e.text}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {warnings.length > 0 && (
                <ul className="ml-4 list-disc text-[color:var(--ss-text-dim)]">
                  {warnings.slice(0, 5).map((w, i) => (
                    <li key={i}>{w.text}</li>
                  ))}
                </ul>
              )}
              {result.notes.length > 0 && (
                <p className="text-[11px] text-[color:var(--ss-text-dim)]">{result.notes.join(" · ")}</p>
              )}
              {q && q.axes.length > 0 && q.outputs.length > 0 ? (
                <div data-import-grid>
                  <p className="mb-1 text-[11px] text-[color:var(--ss-text-dim)]">
                    The table as it will be stored ({result.points.toLocaleString("en")} points
                    {result.filled ? `, ${result.filled.toLocaleString("en")} filled` : ""}):
                  </p>
                  <TableNEditor
                    key={JSON.stringify(result.axes) + build}
                    axes={q.axes}
                    outputs={q.outputs}
                    tables={tables}
                    onChange={setEdited}
                    setUnit={setUnit}
                  />
                </div>
              ) : (
                result.cells.length > 0 && <Cells cells={result.cells} marked={marked} />
              )}
            </>
          )}
        </div>
        <div className="flex items-center justify-end gap-2 border-t border-[color:var(--ss-border)] px-3 py-2">
          <span className="mr-auto text-[11px] text-[color:var(--ss-text-dim)]">
            {result?.ok
              ? build
                ? "Replaces the Map block's inputs, outputs and tables; Undo (Ctrl+Z) brings them back."
                : "Replaces the whole table; Undo (Ctrl+Z) brings it back."
              : ""}
          </span>
          <button className="ss-toolbtn border border-[color:var(--ss-border)] px-2" onClick={onClose}>
            Cancel
          </button>
          <button
            ref={applyRef}
            className="ss-toolbtn border border-[color:var(--ss-accent)] bg-[color:var(--ss-accent)] px-2 text-white disabled:opacity-40"
            disabled={!result?.ok || !result.tables}
            onClick={() =>
              result?.tables &&
              onApply({ tables: (edited ?? result.tables) as Record<string, TableN>, ports: build ? result.ports : null })
            }
          >
            Apply{result?.ok ? ` (${result.points.toLocaleString("en")} points)` : ""}
          </button>
        </div>
      </div>
    </div>
  );
}
