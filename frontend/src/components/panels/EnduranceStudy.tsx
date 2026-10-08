import { useState } from "react";
import { Grid3x3 } from "lucide-react";
import { packKwh } from "../../fsEvents";
import { useProjectStore } from "../../store/projectStore";
import type { SimCase } from "../../types";

const parseList = (text: string) =>
  text
    .split(/[;,\s]+/)
    .map(Number)
    .filter((v) => Number.isFinite(v) && v > 0)
    .slice(0, 6);

const FINISHED = "Endurance finished on its energy";
// the figures the map can show, in order of preference
const CHOICES = [
  "Endurance energy (regeneration × 0.9)",
  "Net battery energy (out − back in)",
  "Endurance time (FS Rules 2026 v1.1 (FSG))",
  "Endurance points (estimate)",
  "Efficiency points (estimate)",
  "RMS battery power",
  "Lowest pack voltage",
];

/** The endurance energy study (STU-38): a grid of the accumulator's size
 *  and its Output Power Limit on an endurance case, shown as a map. */
export function EnduranceStudy({ simCase }: { simCase: SimCase }) {
  const project = useProjectStore((s) => s.project);
  const running = useProjectStore((s) => s.running);
  const runEnduranceStudy = useProjectStore((s) => s.runEnduranceStudy);
  // kept with the project's runs (PLT-34)
  const studies = useProjectStore((s) => s.studies);
  const libraryById = useProjectStore((s) => s.libraryById);
  const battery = project?.systems.flatMap((s) => s.elements).find((e) => e.componentDefId === "battery.generic");
  // its energy as the case runs it (built from cells: from its cells)
  const kwh = packKwh({
    ...Object.fromEntries((libraryById["battery.generic"]?.parameters ?? []).map((d) => [d.key, d.default])),
    ...battery?.parameterOverrides,
    ...simCase.parameterOverrides?.[battery?.id ?? ""],
  });
  const pack = Number.isFinite(kwh) && kwh > 0 ? kwh : 7;
  const [packs, setPacks] = useState(() => [0.8, 0.9, 1, 1.1].map((f) => Math.round(pack * f * 10) / 10).join(", "));
  const [caps, setCaps] = useState("20, 30, 40, 50");
  const study = [...studies]
    .reverse()
    .find(
      (st) =>
        st.caseId === simCase.id &&
        st.factors.length === 2 &&
        st.factors[0].paramKey === "capacity_kWh" &&
        st.factors[1].paramKey === "output_power_limit_kW",
    );
  const labels = study ? CHOICES.filter((c) => study.kpis.some((k) => k.label === c)) : [];
  const [kpi, setKpi] = useState(CHOICES[0]);
  const shown = labels.includes(kpi) ? kpi : labels[0];
  if (!battery) return null;
  const p = parseList(packs);
  const c = parseList(caps);
  const unit = study?.kpis.find((k) => k.label === shown)?.unit ?? "";

  const cell = (pk: number, cp: number) => study?.points.find((pt) => pt.values[0] === pk && pt.values[1] === cp);
  const vals = study
    ? study.points.map((pt) => pt.kpis[shown ?? ""]).filter((v): v is number => typeof v === "number")
    : [];
  const lo = Math.min(...vals);
  const hi = Math.max(...vals);

  return (
    <section className="mb-4" aria-label="Endurance energy study">
      <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
        Endurance energy study
      </div>
      <p className="mb-1 text-[11px] text-[color:var(--ss-text-dim)]">
        Runs this endurance at every pair of {battery.label}&apos;s capacity (its charge in Ah scales with it) and
        Output Power Limit, to see how pack size and power cap trade off. Each run takes about 10 s.
      </p>
      <div className="grid gap-1">
        <label className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]">
          Capacities (kWh)
          <input className="ss-input w-[150px]" value={packs} onChange={(e) => setPacks(e.target.value)} />
        </label>
        <label className="flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]">
          Power limits (kW)
          <input className="ss-input w-[150px]" value={caps} onChange={(e) => setCaps(e.target.value)} />
        </label>
        <button
          className="ss-toolbtn justify-self-end border border-[color:var(--ss-border)]"
          disabled={running || !p.length || !c.length}
          onClick={() => void runEnduranceStudy({ caseId: simCase.id, batteryId: battery.id, packs: p, caps: c })}
          title="Run the grid (at most 6 values each) and save it as a study"
        >
          <Grid3x3 size={12} /> Run {p.length} × {c.length} = {p.length * c.length} runs
        </button>
      </div>
      {study && shown && (
        <div className="mt-2">
          <label className="mb-1 flex items-center gap-1 text-[11px] text-[color:var(--ss-text-dim)]">
            Show
            <select className="ss-input min-w-0 flex-1" value={shown} onChange={(e) => setKpi(e.target.value)}>
              {labels.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <table className="w-full border-collapse text-[11px]" aria-label="Endurance energy map">
            <thead>
              <tr>
                <th className="ss-th text-left">kWh \ kW</th>
                {study.factors[1].values.map((cp) => (
                  <th key={cp} className="ss-th text-right">
                    {cp}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {study.factors[0].values.map((pk) => (
                <tr key={pk}>
                  <th className="ss-th text-left">{pk}</th>
                  {study.factors[1].values.map((cp) => {
                    const pt = cell(pk, cp);
                    const v = pt?.kpis[shown];
                    const dnf = pt?.kpis[FINISHED] === 0;
                    const share = typeof v === "number" && hi > lo ? (v - lo) / (hi - lo) : 0;
                    return (
                      <td
                        key={cp}
                        className="ss-td text-right font-mono"
                        style={{
                          background: dnf
                            ? "color-mix(in srgb, var(--ss-err) 25%, transparent)"
                            : `color-mix(in srgb, var(--ss-accent) ${Math.round(8 + 32 * share)}%, transparent)`,
                        }}
                        title={dnf ? "Did not finish: the pack ran out of energy" : pt?.status}
                      >
                        {typeof v === "number" ? v.toLocaleString("en", { maximumFractionDigits: 3 }) : "—"}
                        {dnf ? " DNF" : ""}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
          <p className="mt-1 text-[10px] text-[color:var(--ss-text-dim)]">
            {unit ? `${shown} in ${unit}. ` : ""}Darker is higher; DNF: the pack ran out before the last lap. The whole
            table is in Saved studies below.
          </p>
        </div>
      )}
    </section>
  );
}
