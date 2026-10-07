import { useMemo, useState } from "react";
import { Play, Plus, Sliders, Square, X } from "lucide-react";
import { useActiveRun, useProjectStore } from "../../store/projectStore";
import { useStaleness } from "./StaleBanner";
import type { ComponentDef, ElementInstance, ParamValue, ParameterDef } from "../../types";
import { StudiesList } from "./StudiesList";
import { CycleSelect } from "./CyclePicker";
import { NumberInput } from "./PropertiesPanel";
import { paramName, rangeProblem } from "../../paramRules";

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

// An acceleration case's own numbers, with the limits the form checks as they
// are typed (UX-10); the engine ignores a value outside them.
const DISTANCE: ParameterDef = {
  key: "endDistance", label: "Distance", unit: "m", default: 75, type: "number", exclusiveMinimum: 0,
};
const START_LINE: ParameterDef = {
  key: "startLine", label: "Start line", unit: "m", default: 0, type: "number", minimum: 0,
};
const REFERENCE_TIME: ParameterDef = {
  key: "referenceTime", label: "Reference time", unit: "s", default: 0, type: "number", exclusiveMinimum: 0,
};

/** A number row of the case settings: red outside its limits, with a line
 *  under it that says why, as a parameter's field is. */
function CaseNumber({
  name,
  title,
  def,
  value,
  onChange,
  onClear,
}: {
  name: string;
  title: string;
  def: ParameterDef;
  value: number | null;
  onChange: (v: number) => void;
  onClear?: () => void;
}) {
  const problem = value == null ? null : rangeProblem(def, value);
  const id = `case-${def.key}-problem`;
  return (
    <>
      <label
        className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
        title={title}
      >
        {name}
        <NumberInput value={value} onChange={onChange} onClear={onClear} def={def} describedBy={id} />
      </label>
      {problem && (
        <div id={id} className="ss-param-problem">
          <span role="alert">
            {paramName(def)} {problem}.
          </span>
        </div>
      )}
    </>
  );
}

/** A scalar value editor matching the parameter type. */
function ValueEditor({
  def,
  value,
  label,
  onChange,
  describedBy,
}: {
  def: ParameterDef;
  value: ParamValue;
  label: string;
  onChange: (v: ParamValue) => void;
  describedBy?: string;
}) {
  if (def.type === "boolean") {
    return (
      <select
        className="ss-input min-w-0"
        aria-label={label}
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
        className="ss-input min-w-0 flex-1"
        aria-label={label}
        title={String(value)}
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
    // turns red outside the limits, and a cleared field is not stored as 0
    return <NumberInput value={Number(value)} onChange={onChange} label={label} def={def} describedBy={describedBy} />;
  }
  return (
    <input
      className="ss-input min-w-0 flex-1"
      aria-label={label}
      title={String(value)}
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
  const setDrivingCycle = useProjectStore((s) => s.setDrivingCycle);
  const run = useProjectStore((s) => s.run);
  const stopRun = useProjectStore((s) => s.stopRun);
  const staleness = useStaleness(useActiveRun());
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

  // cases changed since the run shown in Results (UX-41)
  const staleCases = staleness.caseIds;
  if (!project || !activeCase) {
    return (
      <div className="px-3 py-2 text-[12px] text-[color:var(--ss-text-dim)]">
        No simulation case available.
      </div>
    );
  }

  const accel = activeCase.kind === "acceleration";
  const lap = activeCase.kind === "lap";
  // a lap case drives the model's (one) Race Track: its layout and laps are
  // this case's overrides of it
  const track = project.systems.flatMap((s) => s.elements).find((e) => e.componentDefId === "track.lap");
  const trackParams = libraryById["track.lap"]?.parameters ?? [];
  const layoutDef = trackParams.find((p) => p.key === "layout");
  const lapsDef = trackParams.find((p) => p.key === "laps");
  const lapHint = "A lap case is set by the Race Track's layout and laps: Duration, Step and Pacing do not apply.";
  const overrideRows = Object.entries(caseOv ?? {}).flatMap(([elId, params]) =>
    Object.entries(params).map(([key, value]) => ({ elId, key, value })),
  );

  return (
    <div className="flex h-full flex-col">
      {/* in a narrow panel the Run/Stop buttons wrap below the case */}
      <div className="ss-panel-toolbar flex-wrap">
        <span className="text-[11px] text-[color:var(--ss-text-dim)]">Case</span>
        <select
          className="ss-input w-[150px]"
          aria-label="Case"
          value={activeCase.id}
          onChange={(e) => setActiveCase(e.target.value)}
        >
          {cases.map((c) => (
            <option key={c.id} value={c.id}>
              {staleCases.has(c.id) ? `${c.name} •` : c.name}
            </option>
          ))}
        </select>
        {staleCases.has(activeCase.id) && (
          <span
            className="h-2 w-2 shrink-0 rounded-full bg-[color:var(--ss-accent)]"
            role="img"
            aria-label="Changed since the results shown"
            title="This case changed since the run shown in Results (•)"
          />
        )}
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
        {/* one setting a row: side by side, a side panel left each label ~10 px */}
        <div className="mb-4 grid gap-y-1.5 rounded bg-[color:var(--ss-panel-alt)] p-2">
          <label
            className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
            title={
              accel
                ? "The acceleration test's time limit: a car that has not reached the line by then gets a warning. FS Rules 2026 v1.1 (FSG) D 9.2.1 disqualifies runs over 25 s in driverless runs only; FSUK and FSAE may differ, check the current season's rules."
                : lap
                  ? lapHint
                  : undefined
            }
          >
            Duration (s)
            <input
              type="number"
              className="ss-input disabled:opacity-50"
              disabled={lap}
              min={1}
              value={activeCase.duration}
              onChange={(e) =>
                setCaseField(activeCase.id, { duration: Number(e.target.value) || 1 })
              }
            />
          </label>
          <label
            className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
            title={
              lap
                ? lapHint
                : "Output step in seconds (min 0.0001): results are stored and live edits applied at this interval. Controllers, scripts, the drive cycle and the physics run at the solver step (≤10 ms) whatever this is set to."
            }
          >
            Step (s)
            <input
              type="number"
              className="ss-input disabled:opacity-50"
              disabled={lap}
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
            title={
              lap
                ? "Store a result point every N track points (about 1 m apart; 1 = every point). Keeps a long run's result small: 5 or more for a Formula Student endurance."
                : "Store a result point every N output steps (1 = every step). Keeps results small at fine step sizes."
            }
          >
            Store every (steps)
            <input
              type="number"
              className="ss-input"
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
            title={
              lap
                ? lapHint
                : "0 = solve as fast as possible; N× paces the run against real time so you can watch and tune it live."
            }
          >
            Pacing
            <select
              className="ss-input w-[72px] disabled:opacity-50"
              disabled={lap}
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
          <label
            className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
            title="Cycle: judged on following the target speed. Performance: the Driver holds full throttle until the car reaches the target, then holds it there, and the run reports the time from t = 0 to the target speed and the maximum speed (for example 0:100 for 0-100 km/h, 0:250 for top speed). Acceleration: the Driver holds full throttle the whole run (no target needed) and the run ends at the line, reporting the time from the start line, the speed at the line, 0-100 km/h and the battery's terminal power (results are estimates). Lap: the model's Race Track sets the run (its layout and laps below): a quasi-steady-state lap solver finds the fastest speed along it and the motors and battery drive that speed, reporting the lap and sector times, the energy per lap and what limited the car (results are estimates)."
          >
            Kind
            <select
              className="ss-input w-[112px]"
              value={activeCase.kind ?? "cycle"}
              onChange={(e) => {
                const kind = e.target.value as "cycle" | "performance" | "acceleration" | "lap";
                // the distance end shows only for Acceleration: another kind
                // would end at a line it does not show
                setCaseField(activeCase.id, {
                  kind,
                  endDistance: kind === "acceleration" ? activeCase.endDistance || 75 : null,
                });
              }}
            >
              <option value="cycle">Cycle</option>
              <option value="performance">Performance</option>
              <option value="acceleration">Acceleration</option>
              <option value="lap">Lap</option>
            </select>
          </label>
          <label
            className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
            title="Every run of this case gets an Energy view in Results: where the battery's or fuel's energy went, as a Sankey chart and a table per part, and Energy labels on the diagram."
          >
            Energy report
            <input
              type="checkbox"
              checked={activeCase.energyReport ?? true}
              onChange={(e) => setCaseField(activeCase.id, { energyReport: e.target.checked })}
            />
          </label>
          {lap &&
            (track && layoutDef && lapsDef ? (
              <>
                <label
                  className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]"
                  title={`The layout of Race Track '${track.label}' for this case (a case override). Autocross, Skidpad and Acceleration 75 m are drawn for LightSim after FS Rules 2026 v1.1 (FSG) D 4.1, D 5.1.1, D 6.1 and D 7.1 (FSUK and FSAE may differ, check the current season's rules); Custom uses the track's own tables.`}
                >
                  Track layout
                  <select
                    className="ss-input w-[112px]"
                    value={String(effectiveValue(caseOv, track, "layout", layoutDef))}
                    onChange={(e) => setCaseOverride(activeCase.id, track.id, "layout", e.target.value)}
                  >
                    {(layoutDef.options ?? []).map((o) => (
                      <option key={o} value={o}>
                        {o}
                      </option>
                    ))}
                  </select>
                </label>
                <CaseNumber
                  name="Laps"
                  title="Laps driven one after the other (a case override): lap 1 from the Vehicle's Initial Speed (no faster than the first corner allows), each later lap from the speed the one before ended with."
                  def={lapsDef}
                  value={Number(effectiveValue(caseOv, track, "laps", lapsDef))}
                  onChange={(v) => setCaseOverride(activeCase.id, track.id, "laps", v)}
                />
              </>
            ) : (
              <p className="text-[11px] text-[color:var(--ss-text-dim)]">
                Add a Race Track from Driver &amp; Signals: its layout and laps set a lap case.
              </p>
            ))}
          {accel && (
            <>
              <CaseNumber
                name="Distance (m)"
                title="The run ends at this distance past the start line; the time is taken there (FS Rules 2026 v1.1 (FSG) D 5.1.1: 75 m)."
                def={DISTANCE}
                value={activeCase.endDistance ?? 75}
                onChange={(v) => setCaseField(activeCase.id, { endDistance: v })}
              />
              <CaseNumber
                name="Start line (m)"
                title="The distance the car drives before the timer starts (FS Rules 2026 v1.1 (FSG) D 5.2.3 stages it 0.30 m behind the start line; FSUK and FSAE may differ). 0 = timed from rest. Simulations → Acceleration test adds its case with 0.30 m and a 25 s time limit; a case switched to Acceleration here keeps its own start line and duration."
                def={START_LINE}
                value={activeCase.startLine ?? 0}
                onChange={(v) => setCaseField(activeCase.id, { startLine: v })}
              />
              <CaseNumber
                name="Reference time (s)"
                title="A time to compare with, for example last year's best run: the results show the gap (positive = slower). Empty = none."
                def={REFERENCE_TIME}
                value={activeCase.referenceTime ?? null}
                onChange={(v) => setCaseField(activeCase.id, { referenceTime: v })}
                onClear={() => setCaseField(activeCase.id, { referenceTime: null })}
              />
            </>
          )}
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
          // one block per override, its name on a line of its own and its
          // value below, so a narrow panel shows both without scrolling sideways
          <ul className="mb-2 flex flex-col gap-1">
            {overrideRows.map(({ elId, key, value }) => {
              const ed = elemById.get(elId);
              const pdef = ed?.def.parameters.find((p) => p.key === key);
              const name = `${ed?.el.label ?? elId} · ${pdef?.label ?? key}`;
              const problem = pdef?.type === "number" ? rangeProblem(pdef, Number(value)) : null;
              const problemId = `case-override-${elId}-${key}-problem`;
              return (
                <li
                  key={`${elId}:${key}`}
                  className="rounded border border-[color:var(--ss-border)] px-1.5 py-1"
                >
                  <div className="truncate text-[11px]" title={name}>
                    {name}
                  </div>
                  <div className="mt-0.5 flex min-w-0 items-center gap-1">
                    {pdef && ed && key === "cycle" ? (
                      <CycleSelect
                        value={String(value)}
                        label={name}
                        description={pdef.description}
                        onChange={(v) => setDrivingCycle(elId, v, activeCase.id)}
                      />
                    ) : pdef && ed ? (
                      <ValueEditor
                        def={pdef}
                        value={value}
                        label={name}
                        describedBy={problemId}
                        onChange={(v) => setCaseOverride(activeCase.id, elId, key, v)}
                      />
                    ) : (
                      <span className="truncate text-[11px]">{String(value)}</span>
                    )}
                    {pdef && pdef.unit !== "-" && (
                      <span className="shrink-0 text-[10px] text-[color:var(--ss-text-dim)]">
                        {pdef.unit}
                      </span>
                    )}
                    <button
                      className="ss-toolbtn ml-auto shrink-0 justify-center"
                      title="Remove this override"
                      onClick={() => clearCaseOverride(activeCase.id, elId, key)}
                    >
                      <X size={13} />
                    </button>
                  </div>
                  {pdef && problem && (
                    <div id={problemId} className="ss-param-problem mt-0.5">
                      {/* a lap case's Laps is also in the case settings, which announce it */}
                      <span role={lap && elId === track?.id ? undefined : "alert"}>
                        {paramName(pdef)} {problem}.
                      </span>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
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
              className="ss-input"
              value={swStart}
              step="any"
              onChange={(e) => setSwStart(Number(e.target.value))}
            />
            <span>to</span>
            <input
              type="number"
              className="ss-input"
              value={swStop}
              step="any"
              onChange={(e) => setSwStop(Number(e.target.value))}
            />
            <span>in</span>
            <input
              type="number"
              className="ss-input"
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
