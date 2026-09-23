import { FolderInput } from "lucide-react";
import { confirmReplaceProject, useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import type { SimRun } from "../../types";

/** Run info (RES-09): the model, case settings and version a run was made
 *  with, the live edits made while it ran, and a way to open that model. */
export function RunInfo({ run }: { run: SimRun }) {
  const libraryById = useProjectStore((s) => s.libraryById);
  const openRunModel = useProjectStore((s) => s.openRunModel);
  const snap = run.snapshot;
  if (!snap) {
    return (
      <div role="region" aria-label="Run info" className="rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] px-1.5 py-1 text-[11px] text-[color:var(--ss-text-dim)]">
        This run was stored before runs kept a copy of their model and settings.
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
  const settings = [
    `${c.duration} s`,
    `step ${c.timeStep} s`,
    ...((c.outputEvery ?? 1) > 1 ? [`store ×${c.outputEvery}`] : []),
    (c.realtimeFactor ?? 0) > 0 ? `${c.realtimeFactor}× pacing` : "max speed",
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
        <dd>SimStudio {snap.appVersion ?? "(unknown)"}</dd>
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
