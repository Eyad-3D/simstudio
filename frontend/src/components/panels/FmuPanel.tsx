import { useEffect, useMemo, useRef, useState } from "react";
import { CircleAlert, CircleCheck, FileUp, ShieldCheck, TriangleAlert } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import {
  START_PREFIX,
  allowFmu,
  applyFmuToElement,
  askToAllow,
  describeFmu,
  importFmuFile,
  pinsFor,
  type FmuFile,
  type FmuVariable,
} from "../../fmu";
import type { ElementInstance } from "../../types";

/** How deep a variable sits in the FMU's name tree ("a.b[2].c" is 2). */
function depth(name: string): number {
  return name.replace(/\[[^\]]*\]/g, "").split(".").length - 1;
}

const KIND_TEXT: Record<string, string> = {
  input: "input",
  output: "output",
  parameter: "parameter",
  calculatedParameter: "calculated",
  structuralParameter: "structural",
  local: "internal",
  independent: "time",
};

function Badge({ file }: { file: FmuFile }) {
  const plat = file.info.platform;
  if (!plat) return null;
  const good = plat.runsHere;
  const text = good ? "Runs here" : plat.badge.charAt(0).toUpperCase() + plat.badge.slice(1);
  return (
    <span
      data-testid="fmu-badge"
      className={`rounded px-1.5 py-0.5 text-[10px] font-semibold ${
        good ? "bg-[#e5f5eb] text-emerald-700" : "bg-[#fde8e8] text-[color:var(--ss-err)]"
      }`}
      title={
        plat.operatingSystems.length
          ? `Has compiled code for ${plat.operatingSystems.join(", ")}${plat.hasSources ? ", and source code" : ""}.`
          : plat.hasSources
            ? "Has source code only, no compiled code."
            : "Has no code."
      }
    >
      {text}
    </span>
  );
}

/** The FMU block's own section: its file, what it is, whether it may run on
 *  this computer, and (in the dialog) its variables with a tick to make each
 *  a pin and a start value to change. */
export function FmuPanel({ element, compact }: { element: ElementInstance; compact: boolean }) {
  const setParameter = useProjectStore((s) => s.setParameter);
  const setDynamicPorts = useProjectStore((s) => s.setDynamicPorts);
  const openParamDialog = useUIStore((s) => s.openParamDialog);
  const params = element.parameterOverrides;
  const path = String(params.fmu_path ?? "");
  const sha = String(params.fmu_sha256 ?? "");
  const name = String(params.fmu_name ?? "");
  const [read, setFile] = useState<FmuFile | null>(null);
  // a block whose file was cleared (undo) shows none, whatever was read last
  const file = path || sha ? read : null;
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [filter, setFilter] = useState("");
  const [refresh, setRefresh] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!path && !sha) return;
    let live = true;
    describeFmu({ fmu_path: path, fmu_sha256: sha, fmu_name: name })
      .then((f) => {
        if (!live) return;
        setFile(f);
        setError(null);
      })
      .catch((e: Error) => {
        if (live) setError(`The engine could not read the FMU: ${e.message}`);
      });
    return () => {
      live = false;
    };
  }, [path, sha, name, refresh]);

  const choose = async (picked: File | undefined) => {
    if (!picked) return;
    setBusy(true);
    try {
      const imported = await importFmuFile(picked);
      applyFmuToElement(element.id, imported);
      setFile(imported);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
      if (input.current) input.current.value = "";
    }
  };

  const allow = async () => {
    if (!file) return;
    if (!(await askToAllow(file.name || "this FMU"))) return;
    try {
      await allowFmu(file.sha256, file.name);
      setRefresh((n) => n + 1);
      if (useProjectStore.getState().dataChecks) void useProjectStore.getState().runDataChecks();
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const ticked = useMemo(() => new Set((element.dynamicPorts ?? []).map((p) => p.name)), [element.dynamicPorts]);
  const variables = useMemo(() => file?.info.variables ?? [], [file]);
  const shown = useMemo(() => {
    const t = filter.trim().toLowerCase();
    return t
      ? variables.filter((v) => `${v.name} ${v.description}`.toLowerCase().includes(t))
      : variables.filter((v) => v.causality !== "independent");
  }, [variables, filter]);

  const toggle = (v: FmuVariable, on: boolean) => {
    const next = new Set(ticked);
    if (on) next.add(v.name);
    else next.delete(v.name);
    setDynamicPorts(element.id, pinsFor(element, variables, next));
  };

  const setStart = (v: FmuVariable, text: string) => {
    const t = text.trim();
    const n = t === "" ? v.start : Number(t);
    if (n === null || !Number.isFinite(n)) return;
    const key = START_PREFIX + v.name;
    const now = params[key] ?? v.start;
    if (n !== now) setParameter(element.id, key, n);
  };

  const info = file?.info;
  const pins = (element.dynamicPorts ?? []).length;

  return (
    <div className="flex flex-col gap-1.5" data-param="fmu_variables">
      <div className="text-[11px] font-semibold text-[color:var(--ss-text-dim)]">FMU file</div>
      <input
        ref={input}
        type="file"
        accept=".fmu"
        className="hidden"
        aria-label="FMU file"
        data-testid="fmu-file-input"
        onChange={(e) => void choose(e.target.files?.[0])}
      />
      <button
        className="ss-toolbtn justify-center border border-[color:var(--ss-border)] px-2 py-1"
        disabled={busy}
        onClick={() => input.current?.click()}
      >
        <FileUp size={13} /> {path || sha ? "Choose another FMU file…" : "Choose FMU file…"}
      </button>
      {!path && !sha && !busy && (
        <p className="text-[11px] italic text-[color:var(--ss-text-dim)]">
          Choose a .fmu file exported from another tool, or drop one on the diagram.
        </p>
      )}
      {busy && <p className="text-[11px] text-[color:var(--ss-text-dim)]">Reading the FMU…</p>}
      {error && (
        <p role="alert" className="text-[11px] text-[color:var(--ss-err)]">
          {error}
        </p>
      )}
      {file && !file.found && (
        <p role="alert" className="text-[11px] text-[color:var(--ss-err)]">
          {file.problem}
        </p>
      )}
      {file?.found && info && (
        <div className="flex flex-col gap-1 text-[11px]">
          <div className="flex items-center gap-2">
            <span className="truncate font-semibold" title={file.path}>
              {file.name || info.modelName}
            </span>
            <Badge file={file} />
          </div>
          {info.fmiVersion && (
            <div className="text-[color:var(--ss-text-dim)]">
              FMI {info.fmiVersion} · {(info.kinds ?? []).join(", ") || "unknown kind"}
              {info.generationTool ? ` · made with ${info.generationTool}` : ""}
            </div>
          )}
          {info.description && <div className="text-[color:var(--ss-text-dim)]">{info.description}</div>}
          {file.allowed ? (
            <div className="flex items-center gap-1 text-emerald-700">
              <ShieldCheck size={12} /> Allowed to run on this computer
            </div>
          ) : (
            <button
              className="ss-toolbtn justify-center border border-[color:var(--ss-border)] px-2 py-1"
              onClick={() => void allow()}
            >
              <ShieldCheck size={13} /> Allow this FMU to run…
            </button>
          )}
          {info.problems.map((p) => (
            <p key={p} className="flex gap-1 text-[color:var(--ss-err)]">
              <CircleAlert size={12} className="mt-0.5 shrink-0" /> {p}
            </p>
          ))}
          {(info.warnings ?? []).map((w) => (
            <p key={w} className="flex gap-1 text-[#b45309]">
              <TriangleAlert size={12} className="mt-0.5 shrink-0" /> {w}
            </p>
          ))}
          {info.defaultStepSize ? (
            <div className="text-[color:var(--ss-text-dim)]">
              The FMU suggests a communication step of {info.defaultStepSize} s.
            </div>
          ) : null}
          {compact ? (
            <button
              className="ss-toolbtn justify-between border border-[color:var(--ss-border)] px-2 py-1"
              onClick={() => openParamDialog(element.id, "fmu_variables")}
            >
              <span>Variables and pins</span>
              <span className="text-[10px] text-[color:var(--ss-accent)]">
                {pins} pin{pins === 1 ? "" : "s"} · Edit…
              </span>
            </button>
          ) : (
            <div className="flex flex-col gap-1">
              <div className="flex items-center gap-2">
                <span className="font-semibold text-[color:var(--ss-text-dim)]">Variables</span>
                <span className="text-[10px] text-[color:var(--ss-text-dim)]">
                  tick a variable to make it a pin; change a start value before the run
                </span>
              </div>
              {variables.length > 8 && (
                <input
                  className="ss-input text-[11px]"
                  placeholder="Find a variable"
                  aria-label="Find a variable"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                />
              )}
              <div className="max-h-[320px] overflow-auto rounded border border-[color:var(--ss-border)]">
                <table className="w-full text-[11px]" aria-label="FMU variables">
                  <thead>
                    <tr>
                      <th className="ss-th w-[36px]">Pin</th>
                      <th className="ss-th">Name</th>
                      <th className="ss-th">Kind</th>
                      <th className="ss-th">Start</th>
                      <th className="ss-th">Unit</th>
                    </tr>
                  </thead>
                  <tbody>
                    {shown.map((v) => {
                      const key = START_PREFIX + v.name;
                      const changed = key in params && Number(params[key]) !== v.start;
                      const short = v.name.split(".").pop() ?? v.name;
                      return (
                        <tr key={v.name} title={v.description || undefined}>
                          <td className="ss-td text-center">
                            <input
                              type="checkbox"
                              aria-label={`Pin for ${v.name}`}
                              disabled={!v.pin}
                              checked={ticked.has(v.name)}
                              title={v.pin ? `Make it an ${v.pin} pin` : "Not a signal LightSim can pass (a parameter or not a number)"}
                              onChange={(e) => toggle(v, e.target.checked)}
                            />
                          </td>
                          <td className="ss-td font-mono" style={{ paddingLeft: 4 + 10 * depth(v.name) }}>
                            <span title={v.name}>{filter ? v.name : short}</span>
                          </td>
                          <td className="ss-td whitespace-nowrap text-[color:var(--ss-text-dim)]">
                            {KIND_TEXT[v.causality] ?? v.causality}
                          </td>
                          <td className="ss-td">
                            {v.settable ? (
                              <input
                                key={`${key}:${String(params[key] ?? v.start)}`}
                                className={`ss-input w-[72px] text-right text-[11px] ${changed ? "font-semibold" : ""}`}
                                aria-label={`Start value of ${v.name}`}
                                defaultValue={String(params[key] ?? v.start ?? "")}
                                title={changed ? `Changed from the FMU's ${v.start}` : "The FMU's own start value"}
                                onBlur={(e) => setStart(v, e.target.value)}
                                onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                              />
                            ) : (
                              <span className="text-[color:var(--ss-text-dim)]">{v.start ?? ""}</span>
                            )}
                          </td>
                          <td className="ss-td whitespace-nowrap text-[color:var(--ss-text-dim)]">{v.unit}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
              <div className="flex items-center gap-1 text-[10px] text-[color:var(--ss-text-dim)]">
                <CircleCheck size={11} /> {pins} pin{pins === 1 ? "" : "s"}: wire them on the Data Bus like any
                signal.
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
