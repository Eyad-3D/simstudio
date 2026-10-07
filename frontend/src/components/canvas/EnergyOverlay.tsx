// The diagram's marks from the run shown in Results: a dot on every part
// changed since it (UX-41), and with Energy on, each part's energy in, out
// and lost as a label and a bar chart of the losses (RES-22).
import { useMemo } from "react";
import { X } from "lucide-react";
import { useActiveRun, useProjectStore } from "../../store/projectStore";
import { useReportsStore } from "../../store/reportsStore";
import { useUIStore } from "../../store/uiStore";
import { useStaleness } from "../panels/StaleBanner";
import type { EnergyPart } from "../../types";

const kwh = (v: number) => v.toLocaleString(undefined, { maximumFractionDigits: Math.abs(v) >= 10 ? 1 : 3 });

/** The energy table's row for each part of the run shown in Results. */
function useEnergyParts(): Map<string, EnergyPart> | null {
  const run = useActiveRun();
  const e = run?.result.energy;
  return useMemo(() => (e ? new Map(e.parts.filter((p) => p.elementId).map((p) => [p.elementId!, p])) : null), [e]);
}

/** A part's own marks, drawn inside its node. */
export function NodeMarks({ elementId }: { elementId: string }) {
  const stale = useStaleness(useActiveRun()).elementIds.has(elementId);
  const show = useReportsStore((s) => s.showEnergy);
  const parts = useEnergyParts();
  const p = show ? parts?.get(elementId) : undefined;
  return (
    <>
      {stale && (
        <span
          className="absolute -left-1 -top-1 z-20 h-2.5 w-2.5 rounded-full border border-[color:var(--ss-panel)] bg-[color:var(--ss-accent)]"
          role="img"
          aria-label="Changed since the results shown"
          title="Changed since the run shown in Results: run it again to see what the change does"
        />
      )}
      {p && (
        <div
          className="pointer-events-none absolute left-1/2 top-full z-10 mt-[17px] -translate-x-1/2 whitespace-nowrap rounded px-1 text-[9.5px] leading-tight tabular-nums text-[color:var(--ss-text-dim)]"
          style={{ background: "color-mix(in srgb, var(--ss-panel) 85%, transparent)" }}
          data-energy-label
        >
          in {kwh(p.inKWh)} · out {kwh(p.outKWh)} · lost {kwh(p.lostKWh)} kWh
        </div>
      )}
    </>
  );
}

/** The Energy switch's bar chart: each part's loss, largest first, with a
 *  slider that hides the parts below a share of the sources' energy. A
 *  click on a bar selects the part. */
export function EnergyBars() {
  const show = useReportsStore((s) => s.showEnergy);
  const setShow = useReportsStore((s) => s.setShowEnergy);
  const minPct = useReportsStore((s) => s.energyMinPct);
  const setMinPct = useReportsStore((s) => s.setEnergyMinPct);
  const run = useActiveRun();
  const project = useProjectStore((s) => s.project);
  const e = run?.result.energy;
  if (!show) return null;
  const inProject = new Set(project?.systems.flatMap((sy) => sy.elements.map((x) => x.id)));
  const rows = (e?.parts ?? []).filter((p) => p.lostKWh > 0).sort((a, b) => b.lostKWh - a.lostKWh);
  const shown = rows.filter((p) => p.lostPct >= minPct);
  const top = rows[0]?.lostKWh || 1;
  const select = (id: string) => useUIStore.getState().revealElements?.([id]);
  return (
    <div
      role="region"
      aria-label="Energy lost per part"
      className="absolute right-2 top-2 z-10 w-[280px] rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] p-2 text-[11px] shadow-md"
    >
      <div className="mb-1 flex items-center gap-1">
        <b>Energy lost per part</b>
        <button className="ss-toolbtn ml-auto" title="Hide the energy labels and this chart" onClick={() => setShow(false)}>
          <X size={12} />
        </button>
      </div>
      {!e ? (
        <div className="text-[color:var(--ss-text-dim)]">
          {run ? "The run shown in Results has no energy report: run a case with Energy report ticked." : "Run a case to see where its energy went."}
        </div>
      ) : (
        <>
          <div className="mb-1 truncate text-[10px] text-[color:var(--ss-text-dim)]" title={run?.caseName}>
            {run?.caseName} · sources {kwh(e.sourceKWh)} kWh
          </div>
          <label className="mb-1 flex items-center gap-1 text-[10px] text-[color:var(--ss-text-dim)]">
            Hide below
            <input
              type="range"
              min={0}
              max={10}
              step={0.5}
              value={minPct}
              aria-label="Hide parts below this share of the sources' energy"
              onChange={(ev) => setMinPct(Number(ev.target.value))}
              className="min-w-0 flex-1"
            />
            <span className="w-9 text-right font-mono">{minPct} %</span>
          </label>
          <ul className="max-h-[220px] overflow-y-auto">
            {shown.map((p, i) => {
              const id = p.elementId && inProject.has(p.elementId) ? p.elementId : null;
              return (
                <li key={i}>
                  <button
                    className="group flex w-full items-center gap-1.5 rounded px-0.5 py-[1px] text-left hover:bg-[color:var(--ss-hover)] disabled:cursor-default"
                    disabled={!id}
                    title={`${p.label}: lost ${kwh(p.lostKWh)} kWh, ${p.lostPct.toFixed(1)} % of the sources${id ? " — click to select it" : ""}`}
                    onClick={() => id && select(id)}
                  >
                    <span className="w-[92px] shrink-0 truncate">{p.label}</span>
                    <span className="relative h-2.5 flex-1">
                      <span
                        className="absolute inset-y-0 left-0 rounded-r"
                        style={{ width: `${Math.max(2, (100 * p.lostKWh) / top)}%`, background: "var(--ss-accent)" }}
                      />
                    </span>
                    <span className="w-[72px] shrink-0 text-right font-mono">
                      {kwh(p.lostKWh)} <span className="text-[color:var(--ss-text-dim)]">{p.lostPct.toFixed(1)}%</span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
          {rows.length > shown.length && (
            <div className="mt-1 text-[10px] text-[color:var(--ss-text-dim)]">
              {rows.length - shown.length} part(s) below {minPct} % hidden
            </div>
          )}
        </>
      )}
    </div>
  );
}
