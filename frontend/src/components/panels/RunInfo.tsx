import { useId, useMemo } from "react";
import { FolderInput } from "lucide-react";
import { diffSnapshots } from "../../provenance";
import { confirmReplaceProject, useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import type { SimRun } from "../../types";

/** The clock time a run started. */
export function runTime(r: SimRun): string {
  return new Date(r.startedAt).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** A run in the run pickers: its case, its name (else its clock time) and
 *  an outcome other than success. */
export function runLabel(r: SimRun): string {
  // "success" is the usual case and the chart header shows it: leave it out
  // so the label fits the 280 px picker; any other outcome stays visible
  const outcome = r.incomplete ? `incomplete (${r.incomplete})` : r.status === "success" ? "" : r.status;
  return [r.caseName, r.name ?? runTime(r), outcome].filter(Boolean).join(" · ");
}

/** Compact run label for legends/overlay chips — swept value if present,
 *  else its name or clock time. */
export function runShort(r: SimRun): string {
  const mark = r.incomplete ? " (incomplete)" : "";
  if (r.sweepValue !== undefined)
    return `${r.sweepValue}${r.sweepUnit ? ` ${r.sweepUnit}` : ""}${mark}`;
  return `${r.name ?? runTime(r)}${mark}`;
}

/** Run info (RES-09): the run's name and note (RES-10), the model, case
 *  settings and version it was made with, the live edits made while it ran,
 *  and a way to open that model. */
export function RunInfo({ run }: { run: SimRun }) {
  const libraryById = useProjectStore((s) => s.libraryById);
  const openRunModel = useProjectStore((s) => s.openRunModel);
  const editRun = useProjectStore((s) => s.editRun);
  const running = useProjectStore((s) => s.running);
  const id = useId();
  const snap = run.snapshot;

  // saved when the field is left (Enter too, for the name); keyed by the
  // stored value, so the field shows it as stored (trimmed)
  const fields = (
    <div className="grid grid-cols-[62px_1fr] items-center gap-x-2 gap-y-0.5 border-b border-[color:var(--ss-border)] px-1.5 py-1">
      <label htmlFor={`${id}-name`} className="text-[color:var(--ss-text-dim)]">
        Name
      </label>
      <input
        key={`${run.id}:${run.name ?? ""}`}
        id={`${id}-name`}
        className="ss-input min-w-0"
        maxLength={120}
        placeholder={runTime(run)}
        title="The run's name in the run lists, legends and summary; by default what changed since the previous run of its case"
        defaultValue={run.name ?? ""}
        disabled={running}
        onBlur={(e) => editRun(run.id, { name: e.target.value })}
        onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      />
      <label htmlFor={`${id}-note`} className="self-start pt-0.5 text-[color:var(--ss-text-dim)]">
        Note
      </label>
      <textarea
        key={`${run.id}:${run.note ?? ""}`}
        id={`${id}-note`}
        className="ss-input min-w-0 resize-y"
        rows={2}
        maxLength={4000}
        defaultValue={run.note ?? ""}
        disabled={running}
        onBlur={(e) => editRun(run.id, { note: e.target.value })}
      />
    </div>
  );

  if (!snap) {
    return (
      <div role="region" aria-label="Run info" className="rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] text-[11px]">
        {fields}
        <div className="px-1.5 py-1 text-[color:var(--ss-text-dim)]">
          This run was stored before runs kept a copy of their model and settings.
        </div>
      </div>
    );
  }

  const elements = new Map(snap.project.systems.flatMap((s) => s.elements.map((e) => [e.id, e] as const)));
  const paramName = (elementId: string, key: string) => {
    const el = elements.get(elementId);
    const def = el && libraryById[el.componentDefId]?.parameters.find((p) => p.key === key);
    return `${el?.label ?? elementId} · ${def?.label ?? key}`;
  };
  const c = snap.case;
  const lap = c.kind === "lap"; // the Race Track, not the duration or step, set the run
  const settings = [
    ...(lap ? ["lap mode (estimate)"] : [`${c.duration} s`, `step ${c.timeStep} s`]),
    ...((c.outputEvery ?? 1) > 1 ? [`store ×${c.outputEvery}`] : []),
    ...(lap ? [] : [(c.realtimeFactor ?? 0) > 0 ? `${c.realtimeFactor}× pacing` : "no pacing"]),
    ...(c.kind === "performance" ? ["performance test"] : []),
    ...(c.kind === "acceleration"
      ? [`acceleration test${c.endDistance ? ` over ${c.endDistance} m` : ""}${
          c.startLine ? `, start line ${c.startLine} m` : ""
        } (estimate)`]
      : []),
  ].join(" · ");
  const overrides = Object.entries(c.parameterOverrides ?? {}).flatMap(([elementId, params]) =>
    Object.entries(params).map(([key, value]) =>
      `${paramName(elementId, key)} = ${typeof value === "object" ? "table" : String(value)}`,
    ),
  );
  const elementCount = snap.project.systems.reduce((n, s) => n + s.elements.length, 0);

  const openModel = () =>
    void confirmReplaceProject("Opening the run's model").then((ok) => {
      if (!ok) return;
      openRunModel(run.id);
      useUIStore.getState().setRibbonTab("home");
    });

  return (
    <div role="region" aria-label="Run info" className="max-h-[260px] overflow-y-auto rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] text-[11px]">
      {fields}
      <dl className="grid grid-cols-[62px_1fr] gap-x-2 gap-y-0.5 px-1.5 py-1 [&>dd]:min-w-0 [&>dd]:break-words [&>dt]:text-[color:var(--ss-text-dim)]">
        <dt>Case</dt>
        <dd>{c.name}</dd>
        <dt>Settings</dt>
        <dd>{settings}</dd>
        <dt>Overrides</dt>
        <dd>{overrides.length > 0 ? overrides.join("; ") : "none"}</dd>
        <dt>Model</dt>
        <dd title={snap.modelHash ? `SHA-256 ${snap.modelHash}` : undefined}>
          {snap.project.name} · {elementCount} element(s)
          {snap.modelHash && <span className="font-mono text-[color:var(--ss-text-dim)]"> · #{snap.modelHash.slice(0, 12)}</span>}
        </dd>
        <dt>Version</dt>
        <dd>LightSim {snap.appVersion ?? "(unknown)"}</dd>
        <dt>Live edits</dt>
        <dd>
          {snap.liveEdits.length === 0 ? (
            "none"
          ) : (
            <ul>
              {snap.liveEdits.map((e, i) => (
                <li key={i}>
                  t ≈ {e.t.toLocaleString(undefined, { maximumFractionDigits: 1 })} s: {paramName(e.elementId, e.key)} ={" "}
                  {String(e.value)}
                </li>
              ))}
            </ul>
          )}
        </dd>
      </dl>
      <div className="border-t border-[color:var(--ss-border)] px-1.5 py-1">
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5"
          title="Open the model this run was made with, as it was when the run started, as an unsaved copy"
          onClick={openModel}
        >
          <FolderInput size={12} /> Open as model
        </button>
      </div>
    </div>
  );
}

/** What changed from the baseline run to this one (RES-10): parameters old →
 *  new, maps and scripts edited, parts, wiring, case settings and both runs'
 *  live edits. A line about a part of the open project shows it on the
 *  diagram. */
export function RunChanges({ run, base }: { run: SimRun; base: SimRun }) {
  const libraryById = useProjectStore((s) => s.libraryById);
  const project = useProjectStore((s) => s.project);
  const changes = useMemo(
    () => (run.snapshot && base.snapshot ? diffSnapshots(base.snapshot, run.snapshot, libraryById) : null),
    [run.snapshot, base.snapshot, libraryById],
  );
  const inProject = new Set(project?.systems.flatMap((s) => s.elements.map((e) => e.id)));
  // as the Problems list does: Home, the part selected and framed
  const show = (elementId: string) => {
    const ui = useUIStore.getState();
    ui.setRibbonTab("home");
    ui.focusPanel("topology");
    ui.revealElements?.([elementId]);
  };
  return (
    <div
      role="region"
      aria-label="What changed"
      tabIndex={0}
      className="max-h-[96px] overflow-y-auto rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] px-1.5 py-1 text-[11px]"
    >
      <div className="text-[10px] text-[color:var(--ss-text-dim)]">What changed since {runShort(base)}</div>
      {!changes ? (
        <div className="text-[color:var(--ss-text-dim)]">
          This run or the baseline was stored before runs kept their model.
        </div>
      ) : changes.length === 0 ? (
        <div className="text-[color:var(--ss-text-dim)]">Same model and case settings.</div>
      ) : (
        <ul>
          {changes.map((c, i) => (
            <li key={i} className="break-words">
              {c.elementId && inProject.has(c.elementId) ? (
                <button
                  className="text-left text-[color:var(--ss-accent)] hover:underline"
                  title="Show this part on the diagram"
                  onClick={() => show(c.elementId!)}
                >
                  {c.text}
                </button>
              ) : (
                c.text
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
