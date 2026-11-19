import { useEffect, useId, useRef, useState } from "react";
import { FileUp, X } from "lucide-react";
import * as api from "../api";
import { useProjectStore } from "../store/projectStore";
import type { ParamValue } from "../types";

/** A value in the change list: numbers and text as they are, a table as its
 *  number of points. */
function shown(v: ParamValue): string {
  if (v && typeof v === "object") {
    const n = Object.values(v).reduce<number>((k, x) => k + (x && typeof x === "object" ? Object.keys(x).length : 1), 0);
    return `table (${n} points)`;
  }
  if (typeof v === "string" && v.includes(":") && v.includes(";")) return `profile (${v.split(";").length} points)`;
  if (typeof v === "boolean") return v ? "on" : "off";
  return String(v);
}

async function download(get: () => Promise<{ blob: Blob; name: string }>) {
  try {
    const { blob, name } = await get();
    api.downloadBlob(blob, name);
  } catch (e) {
    useProjectStore.getState().log("error", `The parameter sheet could not be made: ${(e as Error).message}`);
  }
}

/** Export every parameter to one spreadsheet, and import it back with a
 *  list of the changes before they are applied (STD-36). The caller draws
 *  the buttons; `elements` (the file picker and the preview) goes next to them. */
export function useParameterSheet() {
  const input = useRef<HTMLInputElement>(null);
  const [preview, setPreview] = useState<{ name: string; result: api.ParameterSheetImport } | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const exportAs = (format: "xlsx" | "csv") => {
    const p = useProjectStore.getState().project;
    if (p) void download(() => api.exportParameterSheet(p, format));
  };
  const elements = (
    <>
      <input
        ref={input}
        type="file"
        accept=".xlsx,.xlsm,.csv,text/csv"
        className="hidden"
        aria-label="Parameter sheet to import"
        onChange={async (e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          const p = useProjectStore.getState().project;
          if (!f || !p) return;
          try {
            const result = await api.importParameterSheet(p, { name: f.name, data: await api.fileToBase64(f) });
            setFailure(null);
            setPreview({ name: f.name, result });
          } catch (err) {
            setFailure((err as Error).message.replace(/^\d+ /, ""));
            setPreview({ name: f.name, result: { ok: false, changes: [], errors: [], warnings: [], rows: 0, unchanged: 0 } });
          }
        }}
      />
      {preview && (
        <SheetPreview name={preview.name} result={preview.result} failure={failure} onClose={() => setPreview(null)} />
      )}
    </>
  );
  return {
    exportXlsx: () => exportAs("xlsx"),
    exportCsv: () => exportAs("csv"),
    pickFile: () => input.current?.click(),
    elements,
  };
}

function SheetPreview({
  name,
  result,
  failure,
  onClose,
}: {
  name: string;
  result: api.ParameterSheetImport;
  failure: string | null;
  onClose: () => void;
}) {
  const apply = useProjectStore((s) => s.applyParameterChanges);
  const titleId = useId();
  const applyRef = useRef<HTMLButtonElement>(null);
  const n = result.changes.length;
  useEffect(() => {
    applyRef.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div
      className="ss-import-dialog fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="flex max-h-[86vh] w-[min(680px,calc(100vw-32px))] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="flex items-center gap-2 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-3 py-2">
          <FileUp size={15} className="text-[color:var(--ss-accent)]" />
          <span id={titleId} className="min-w-0 truncate text-[13px] font-semibold">
            Import parameters from {name}
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
          {!failure && (
            <p>
              {result.rows} row{result.rows === 1 ? "" : "s"} read: {n} change{n === 1 ? "" : "s"}, {result.unchanged}{" "}
              unchanged{result.errors.length ? `, ${result.errors.length} problem${result.errors.length === 1 ? "" : "s"}` : ""}.
            </p>
          )}
          {result.errors.length > 0 && (
            <div role="alert">
              <p className="font-semibold">Nothing is applied until these rows are fixed in the file:</p>
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
              {result.warnings.map((w, i) => (
                <li key={i}>{w.text}</li>
              ))}
            </ul>
          )}
          {n > 0 && (
            <table className="w-full border-collapse text-[11px]">
              <caption className="text-left font-semibold">What changes</caption>
              <thead>
                <tr className="text-left text-[color:var(--ss-text-dim)]">
                  <th className="border-b border-[color:var(--ss-border)] px-1 font-normal">Row</th>
                  <th className="border-b border-[color:var(--ss-border)] px-1 font-normal">Part · Parameter</th>
                  <th className="border-b border-[color:var(--ss-border)] px-1 font-normal">Now</th>
                  <th className="border-b border-[color:var(--ss-border)] px-1 font-normal">From the sheet</th>
                </tr>
              </thead>
              <tbody>
                {result.changes.map((c) => (
                  <tr key={`${c.elementId}.${c.key}`}>
                    <td className="px-1 align-top text-[color:var(--ss-text-dim)]">
                      {c.sheet && c.sheet !== "Parameters" && !c.sheet.endsWith(".csv") ? `'${c.sheet}'` : c.row}
                    </td>
                    <td className="px-1 align-top">
                      {c.element} · {c.parameter}
                    </td>
                    <td className="px-1 align-top">
                      {shown(c.old)} {c.unit !== "-" && typeof c.old !== "object" ? c.unit : ""}
                    </td>
                    <td className="px-1 align-top font-semibold">
                      {shown(c.new)} {c.unit !== "-" && typeof c.new !== "object" ? c.unit : ""}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <div className="flex items-center justify-end gap-2 border-t border-[color:var(--ss-border)] px-3 py-2">
          <span className="mr-auto text-[11px] text-[color:var(--ss-text-dim)]">
            {result.ok && n > 0 ? "Applied as one step: Undo (Ctrl+Z) takes it all back." : ""}
          </span>
          <button className="ss-toolbtn border border-[color:var(--ss-border)] px-2" onClick={onClose}>
            {result.ok && n > 0 ? "Cancel" : "Close"}
          </button>
          {result.ok && n > 0 && (
            <button
              ref={applyRef}
              className="ss-toolbtn border border-[color:var(--ss-accent)] bg-[color:var(--ss-accent)] px-2 text-white"
              onClick={() => {
                apply(result.changes.map((c) => ({ elementId: c.elementId, key: c.key, value: c.new })));
                useProjectStore
                  .getState()
                  .log("info", `Applied ${n} parameter change${n === 1 ? "" : "s"} from ${name}.`);
                onClose();
              }}
            >
              Apply {n} change{n === 1 ? "" : "s"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/** Save the Formula Student template sheet. */
export function downloadParameterTemplate() {
  void download(api.fetchParameterTemplate);
}
