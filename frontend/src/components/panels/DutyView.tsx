// The Results page's Duty view (RES-39): each motor's, battery's, engine's,
// fuel cell's and DC-DC converter's highest, lowest, mean and RMS power,
// torque and current, which set their cooling and sizing, and the time
// spent above a power you pick.
import { useMemo, useState } from "react";
import { Download } from "lucide-react";
import { DUTY_CHANNEL, downloadText, dutyCsv, timeAbove } from "../../reports";
import type { SimRun } from "../../types";

const num = (v: number) => v.toLocaleString(undefined, { maximumFractionDigits: Math.abs(v) >= 100 ? 1 : 2 });

export function DutyView({ run }: { run: SimRun }) {
  const duty = run.result.duty;
  const [threshold, setThreshold] = useState(60);
  // time above the threshold, from the stored points of each power channel
  const above = useMemo(() => {
    const out: Record<string, number | null> = {};
    for (const d of duty ?? [])
      for (const r of d.rows) {
        if (r.unit !== "kW") continue;
        const port = DUTY_CHANNEL[d.kind]?.[r.quantity];
        const ch = port && run.result.channels.find((c) => c.elementId === d.elementId && c.portId === port);
        out[`${d.elementId}:${r.quantity}`] = ch ? timeAbove(ch.timeSeries, threshold) : null;
      }
    return out;
  }, [duty, run.result.channels, threshold]);

  if (!duty || duty.length === 0) {
    return (
      <div className="flex h-full min-h-[200px] items-center justify-center px-4 text-center text-[12px] text-[color:var(--ss-text-dim)]">
        {run.status === "running"
          ? "The duty table comes when the run finishes."
          : "This run has no duty table: it has no motor, battery, engine, fuel cell or DC-DC converter, it failed, or it was made before LightSim 0.3."}
      </div>
    );
  }
  const step = run.snapshot?.case.timeStep;
  const every = run.snapshot?.case.outputEvery ?? 1;
  const stored = step ? `${(step * every).toLocaleString()} s` : "the stored step";

  return (
    <div className="flex min-h-[260px] flex-[3] flex-col overflow-auto">
      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 px-2 py-1 text-[11px] text-[color:var(--ss-text-dim)]">
        <span title="RMS, the root-mean-square, is the mean that sets heating: a part's losses grow with the square of its current, so its cooling is sized from the RMS, not the peak.">
          Highest, lowest, mean and RMS over the run, from the solver's own steps
        </span>
        <label className="flex items-center gap-1" title={`Time a power was above this, from the stored points (one every ${stored})`}>
          Time above
          <input
            type="number"
            className="ss-input w-[64px] py-0.5"
            aria-label="Time above, kW"
            value={threshold}
            onChange={(e) => setThreshold(Number(e.target.value) || 0)}
          />
          kW
        </label>
        <button
          className="ss-toolbtn ml-auto border border-[color:var(--ss-border)]"
          title="Save the duty table as CSV"
          onClick={() => downloadText(dutyCsv(duty, above), `lightsim-${run.caseName}-duty.csv`)}
        >
          <Download size={12} /> CSV
        </button>
      </div>
      <div className="px-2 pb-2">
        <table className="w-full border-collapse" aria-label="Duty per part">
          <thead>
            <tr>
              <th className="ss-th">Part · quantity</th>
              <th className="ss-th text-right">Highest</th>
              <th className="ss-th text-right">Lowest</th>
              <th className="ss-th text-right">Mean</th>
              <th className="ss-th text-right" title="Root-mean-square: the mean that sets heating">RMS</th>
              <th className="ss-th">Unit</th>
              <th className="ss-th text-right" title={`From the stored points (one every ${stored})`}>
                Time above {threshold.toLocaleString()} kW [s]
              </th>
            </tr>
          </thead>
          <tbody>
            {duty.map((d) =>
              d.rows.map((r, i) => {
                const t = above[`${d.elementId}:${r.quantity}`];
                return (
                  <tr key={`${d.elementId}:${r.quantity}`} className="hover:bg-[color:var(--ss-hover)]">
                    <td className="ss-td">
                      {i === 0 ? <b>{d.label}</b> : null}
                      {i === 0 ? " · " : <span className="pl-3" />}
                      {r.quantity}
                    </td>
                    <td className="ss-td text-right font-mono">{num(r.max)}</td>
                    <td className="ss-td text-right font-mono">{num(r.min)}</td>
                    <td className="ss-td text-right font-mono">{num(r.mean)}</td>
                    <td className="ss-td text-right font-mono font-semibold">{num(r.rms)}</td>
                    <td className="ss-td text-[color:var(--ss-text-dim)]">{r.unit}</td>
                    <td className="ss-td text-right font-mono">{t == null ? "—" : num(t)}</td>
                  </tr>
                );
              }),
            )}
          </tbody>
        </table>
        <p className="mt-1 text-[10px] text-[color:var(--ss-text-dim)]">
          A motor's DC current is its electrical power over its bus voltage (the phase current is not modelled). Each
          part's RMS and peak power, torque and current can be a study's column (Parameter studies).
        </p>
      </div>
    </div>
  );
}
