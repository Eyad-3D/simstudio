import { useMemo, useState } from "react";
import { Play, Plus, Sliders, Square, X } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import type { ComponentDef, ElementInstance, ParamValue, ParameterDef } from "../../types";
import { StudiesList } from "./StudiesList";

// Only scalar parameters are editable as per-case overrides here; tables and
// code are edited in Properties. Sweeps additionally require a numeric param.
const SCALAR_TYPES = new Set(["number", "enum", "boolean", "string"]);

interface ElemDef {
  el: ElementInstance;
  def: ComponentDef;
}

function scalarParams(def: ComponentDef): ParameterDef[] {
  return def.parameters.filter((p) => SCALAR_TYPES.has(p.type));
}

/** value shown in an override editor: case override → element override → default */
function effectiveValue(
  caseOv: Record<string, Record<string, ParamValue>> | undefined,
  el: ElementInstance,
  key: string,
  def: ParameterDef,
): ParamValue {
  return caseOv?.[el.id]?.[key] ?? el.parameterOverrides[key] ?? def.default;
}

function linspace(start: number, stop: number, steps: number): number[] {
  const n = Math.max(1, Math.min(16, Math.round(steps)));
  if (n === 1) return [round(start)];
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(round(start + ((stop - start) * i) / (n - 1)));
  return out;
}

function round(v: number): number {
  return Math.round(v * 1e6) / 1e6;
}

/** A scalar value editor matching the parameter type. */
function ValueEditor({
  def,
  value,
  onChange,
}: {
  def: ParameterDef;
  value: ParamValue;
  onChange: (v: ParamValue) => void;
}) {
  if (def.type === "boolean") {
    return (
      <select
        className="ss-input w-[110px]"
        value={String(value)}
        onChange={(e) => onChange(e.target.value === "true")}
      >
        <option value="true">true</option>
        <option value="false">false</option>
      </select>
    );
  }
  if (def.type === "enum") {
    return (
      <select
        className="ss-input w-[130px]"
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
  }
  if (def.type === "number") {
    return (
      <input
        type="number"
        className="ss-input w-[110px]"
        value={Number(value)}
        step="any"
        onChange={(e) => onChange(Number(e.target.value))}
      />
    );
  }
  return (
    <input
      className="ss-input w-[130px]"
      value={String(value)}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

export function CasePanel() {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const activeCaseId = useProjectStore((s) => s.activeCaseId);
  const running = useProjectStore((s) => s.running);
  const setActiveCase = useProjectStore((s) => s.setActiveCase);
  const setCaseField = useProjectStore((s) => s.setCaseField);
  const setCaseOverride = useProjectStore((s) => s.setCaseOverride);
  const clearCaseOverride = useProjectStore((s) => s.clearCaseOverride);
  const run = useProjectStore((s) => s.run);
  const stopRun = useProjectStore((s) => s.stopRun);
  const runSweep = useProjectStore((s) => s.runSweep);

  const cases = project?.cases ?? [];
  const activeCase = cases.find((c) => c.id === activeCaseId) ?? cases[0];
  const caseOv = activeCase?.parameterOverrides;

  // elements (across all systems) that carry at least one scalar parameter
  const elems = useMemo<ElemDef[]>(() => {
    if (!project) return [];
    return project.systems
      .flatMap((s) => s.elements)
      .map((el) => ({ el, def: libraryById[el.componentDefId] }))
      .filter((ed): ed is ElemDef => Boolean(ed.def) && scalarParams(ed.def).length > 0);
  }, [project, libraryById]);
  const elemById = useMemo(() => new Map(elems.map((e) => [e.el.id, e])), [elems]);

  // ---- add-override form ----------------------------------------------------
  const [ovEl, setOvEl] = useState("");
  const [ovKey, setOvKey] = useState("");
  const ovElDef = elemById.get(ovEl);
  const ovParams = ovElDef ? scalarParams(ovElDef.def) : [];
  const ovParam = ovParams.find((p) => p.key === ovKey) ?? ovParams[0];

  const addOverride = () => {
    if (!activeCase || !ovElDef || !ovParam) return;
    const cur = effectiveValue(caseOv, ovElDef.el, ovParam.key, ovParam);
    setCaseOverride(activeCase.id, ovElDef.el.id, ovParam.key, cur);
  };

  // ---- sweep form -----------------------------------------------------------
  const numericElems = useMemo(
    () => elems.filter((ed) => ed.def.parameters.some((p) => p.type === "number")),
    [elems],
  );
  const [swEl, setSwEl] = useState("");
  const [swKey, setSwKey] = useState("");
  const [swStart, setSwStart] = useState(0);
  const [swStop, setSwStop] = useState(0);
  const [swSteps, setSwSteps] = useState(5);
  const swElDef = elemById.get(swEl);
  const swParams = swElDef ? swElDef.def.parameters.filter((p) => p.type === "number") : [];
  const swParam = swParams.find((p) => p.key === swKey) ?? swParams[0];
  const sweepValues = useMemo(
    () => linspace(swStart, swStop, swSteps),
    [swStart, swStop, swSteps],
  );

  const startSweep = () => {
    if (!activeCase || !swElDef || !swParam) return;
    void runSweep({
      caseId: activeCase.id,
      elementId: swElDef.el.id,
      paramKey: swParam.key,
      values: sweepValues,
    });
  };

  // seed a sweep range from the parameter's current value when it changes
  const seedSweep = (edId: string, key: string) => {
    const ed = elemById.get(edId);
    const pdef = ed?.def.parameters.find((p) => p.key === key && p.type === "number");
    if (!ed || !pdef) return;
    const base = Number(effectiveValue(caseOv, ed.el, key, pdef)) || 0;
    setSwStart(round(base * 0.5));
    setSwStop(round(base * 1.5 || 1));
  };

  if (!project || !activeCase) {
    return (
      <div className="px-3 py-2 text-[12px] text-[color:var(--ss-text-dim)]">
        No simulation case available.
      </div>
    );
  }

  const overrideRows = Object.entries(caseOv ?? {}).flatMap(([elId, params]) =>
    Object.entries(params).map(([key, value]) => ({ elId, key, value })),
  );

  return (
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar">
        <span className="text-[11px] text-[color:var(--ss-text-dim)]">Case</span>
        <select
          className="ss-input w-[150px]"
          value={activeCase.id}
          onChange={(e) => setActiveCase(e.target.value)}
        >
          {cases.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
        <div className="ml-auto flex items-center gap-1">
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)]"
            disabled={running}
            onClick={() => void run()}
            title="Run this case with its overrides"
          >
            <Play size={13} /> Run case
          </button>
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] disabled:opacity-40"
            disabled={!running}
            onClick={stopRun}
            title="Stop the running simulation / sweep"
          >
            <Square size={13} /> Stop
          </button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {/* -- case settings ------------------------------------------------ */}
        <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
          Case settings
        </div>
        <div className="mb-4 grid grid-cols-2 gap-x-3 gap-y-1.5 rounded bg-[color:var(--ss-panel-alt)] p-2">
          <label className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]">
            Duration (s)
            <input
              type="number"
              className="ss-input w-[80px]"
              min={1}
              value={activeCase.duration}
              onChange={(e) =>
                setCaseField(activeCase.id, { duration: Number(e.target.value) || 1 })
              }
            />
          </label>
          <label
            className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
            title="Output step in seconds (min 0.0001): results are stored and live edits applied at this interval. Controllers, scripts, the drive cycle and the physics run at the solver step (≤10 ms) whatever this is set to."
          >
            Step (s)
            <input
              type="number"
              className="ss-input w-[80px]"
              step="any"
              min={0.0001}
              value={activeCase.timeStep}
              onChange={(e) =>
                setCaseField(activeCase.id, {
                  timeStep: Math.max(0.0001, Number(e.target.value) || 1),
                })
              }
            />
          </label>
          <label
            className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
            title="Store a result point every N output steps (1 = every step). Keeps results small at fine step sizes."
          >
            Store every (steps)
            <input
              type="number"
              className="ss-input w-[80px]"
              min={1}
              step={1}
              value={activeCase.outputEvery ?? 1}
              onChange={(e) =>
                setCaseField(activeCase.id, {
                  outputEvery: Math.max(1, Math.round(Number(e.target.value) || 1)),
                })
              }
            />
          </label>
          <label
            className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
            title="0 = solve as fast as possible; N× paces the run against real time so you can watch and tune it live."
          >
            Pacing
            <select
              className="ss-input w-[80px]"
              value={activeCase.realtimeFactor ?? 0}
              onChange={(e) =>
                setCaseField(activeCase.id, { realtimeFactor: Number(e.target.value) })
              }
            >
              <option value={0}>Max</option>
              <option value={1}>1×</option>
              <option value={5}>5×</option>
              <option value={10}>10×</option>
              <option value={30}>30×</option>
            </select>
          </label>
        </div>

        {/* -- per-case overrides ------------------------------------------- */}
        <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
          Parameter overrides · {activeCase.name}
        </div>
        <p className="mb-2 text-[11px] text-[color:var(--ss-text-dim)]">
          Overrides apply only when this case runs — the shared topology and its
          element parameters stay untouched.
        </p>

        {overrideRows.length > 0 ? (
          <table className="mb-2 w-full border-collapse">
            <thead>
              <tr>
                <th className="ss-th">Element</th>
                <th className="ss-th">Parameter</th>
                <th className="ss-th w-[120px]">Value</th>
                <th className="ss-th w-[30px]"></th>
              </tr>
            </thead>
            <tbody>
              {overrideRows.map(({ elId, key, value }) => {
                const ed = elemById.get(elId);
                const pdef = ed?.def.parameters.find((p) => p.key === key);
                return (
                  <tr key={`${elId}:${key}`}>
                    <td className="ss-td truncate">{ed?.el.label ?? elId}</td>
                    <td className="ss-td truncate">{pdef?.label ?? key}</td>
                    <td className="ss-td">
                      {pdef && ed ? (
                        <ValueEditor
                          def={pdef}
                          value={value}
                          onChange={(v) => setCaseOverride(activeCase.id, elId, key, v)}
                        />
                      ) : (
                        String(value)
                      )}
                      {pdef && pdef.unit !== "-" && (
                        <span className="ml-1 text-[10px] text-[color:var(--ss-text-dim)]">
                          {pdef.unit}
                        </span>
                      )}
                    </td>
                    <td className="ss-td text-center">
                      <button
                        className="ss-toolbtn justify-center"
                        title="Remove this override"
                        onClick={() => clearCaseOverride(activeCase.id, elId, key)}
                      >
                        <X size={13} />
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        ) : (
          <div className="mb-2 rounded border border-dashed border-[color:var(--ss-border)] px-2 py-1.5 text-[11px] text-[color:var(--ss-text-dim)]">
            No overrides — this case uses the base parameters.
          </div>
        )}

        {/* add override */}
        <div className="mb-4 flex flex-wrap items-center gap-1 rounded bg-[color:var(--ss-panel-alt)] p-1.5">
          <select
            className="ss-input w-[150px]"
            value={ovEl}
            onChange={(e) => {
              setOvEl(e.target.value);
              setOvKey("");
            }}
          >
            <option value="">Element…</option>
            {elems.map(({ el }) => (
              <option key={el.id} value={el.id}>
                {el.label}
              </option>
            ))}
          </select>
          <select
            className="ss-input w-[150px]"
            value={ovParam?.key ?? ""}
            disabled={!ovElDef}
            onChange={(e) => setOvKey(e.target.value)}
          >
            {ovParams.map((p) => (
              <option key={p.key} value={p.key}>
                {p.label}
                {p.unit !== "-" ? ` (${p.unit})` : ""}
              </option>
            ))}
          </select>
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] disabled:opacity-40"
            disabled={!ovElDef || !ovParam}
            onClick={addOverride}
            title="Add this parameter as a case override"
          >
            <Plus size={13} /> Add override
          </button>
        </div>

        {/* -- parameter sweep --------------------------------------------- */}
        <div className="mb-1 flex items-center gap-1 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
          <Sliders size={12} /> Parameter sweep
        </div>
        <p className="mb-2 text-[11px] text-[color:var(--ss-text-dim)]">
          Run this case once per value of one numeric parameter. Each run lands in
          the Results history so you can overlay and compare them.
        </p>

        <div className="rounded bg-[color:var(--ss-panel-alt)] p-1.5">
          <div className="mb-1.5 flex flex-wrap items-center gap-1">
            <select
              className="ss-input w-[150px]"
              value={swEl}
              onChange={(e) => {
                setSwEl(e.target.value);
                setSwKey("");
                const first = elemById
                  .get(e.target.value)
                  ?.def.parameters.find((p) => p.type === "number");
                if (first) seedSweep(e.target.value, first.key);
              }}
            >
              <option value="">Element…</option>
              {numericElems.map(({ el }) => (
                <option key={el.id} value={el.id}>
                  {el.label}
                </option>
              ))}
            </select>
            <select
              className="ss-input w-[150px]"
              value={swParam?.key ?? ""}
              disabled={!swElDef}
              onChange={(e) => {
                setSwKey(e.target.value);
                seedSweep(swEl, e.target.value);
              }}
            >
              {swParams.map((p) => (
                <option key={p.key} value={p.key}>
                  {p.label}
                  {p.unit !== "-" ? ` (${p.unit})` : ""}
                </option>
              ))}
            </select>
          </div>
          <div className="mb-1.5 flex flex-wrap items-center gap-1 text-[11px] text-[color:var(--ss-text-dim)]">
            <span>From</span>
            <input
              type="number"
              className="ss-input w-[80px]"
              value={swStart}
              step="any"
              onChange={(e) => setSwStart(Number(e.target.value))}
            />
            <span>to</span>
            <input
              type="number"
              className="ss-input w-[80px]"
              value={swStop}
              step="any"
              onChange={(e) => setSwStop(Number(e.target.value))}
            />
            <span>in</span>
            <input
              type="number"
              className="ss-input w-[56px]"
              value={swSteps}
              min={1}
              max={16}
              step={1}
              onChange={(e) => setSwSteps(Math.max(1, Math.min(16, Math.round(Number(e.target.value) || 1))))}
            />
            <span>steps</span>
          </div>
          {swParam && (
            <div className="mb-1.5 flex flex-wrap gap-1">
              {sweepValues.map((v, i) => (
                <span
                  key={i}
                  className="rounded bg-[color:var(--ss-accent-soft)] px-1.5 py-0.5 text-[10px] text-[color:var(--ss-text)]"
                >
                  {v}
                  {swParam.unit !== "-" ? ` ${swParam.unit}` : ""}
                </span>
              ))}
            </div>
          )}
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] disabled:opacity-40"
            disabled={running || !swElDef || !swParam || sweepValues.length === 0}
            onClick={startSweep}
            title="Run the sweep"
          >
            <Play size={13} /> Run sweep ({sweepValues.length})
          </button>
        </div>

        <StudiesList />
      </div>
    </div>
  );
}
