import { Trophy } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { FS_EVENT_NAMES, fsPointsTable } from "../../fsEvents";

const fmt = (v: number | undefined, d: number) =>
  v == null
    ? "—"
    : v.toLocaleString("en-GB", {
        minimumFractionDigits: d,
        maximumFractionDigits: d,
      });

/** The Formula Student points of the newest run of each event's case
 *  (MOD-43), shown once a case is marked for an event. */
export function FsPoints() {
  const project = useProjectStore((s) => s.project);
  const runs = useProjectStore((s) => s.runs);
  const running = useProjectStore((s) => s.running);
  const runFsEvents = useProjectStore((s) => s.runFsEvents);
  if (!project || !project.cases.some((c) => c.fsEvent)) return null;
  const { rows, total } = fsPointsTable(project, runs);
  const max = rows.reduce((a, r) => a + r.maxPoints, 0);
  return (
    <section className="mb-4" aria-label="Formula Student points">
      <div className="mb-1 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
        Formula Student points
        <button
          className="ss-toolbtn ml-auto border border-[color:var(--ss-border)] normal-case"
          disabled={running}
          onClick={() => void runFsEvents()}
          title="Run the four event cases again and update the points"
        >
          <Trophy size={12} /> Run events
        </button>
      </div>
      <table className="w-full border-collapse text-[11px]" aria-label="Formula Student points table">
        <thead>
          <tr className="text-left text-[color:var(--ss-text-dim)]">
            <th className="py-0.5 pr-2 font-normal">Event</th>
            <th className="py-0.5 pr-2 text-right font-normal">Time (s)</th>
            <th className="py-0.5 text-right font-normal">Points</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.event} className="border-t border-[color:var(--ss-border)] align-top">
              <td className="py-0.5 pr-2" title={r.caseName ? `Case '${r.caseName}'` : undefined}>
                {FS_EVENT_NAMES[r.event]}
                {(r.note || r.breach) && (
                  <div className="text-[10px] text-[color:var(--ss-text-dim)]">
                    {r.breach ? `Rule broken: ${r.breach}. ` : ""}
                    {r.note}
                  </div>
                )}
              </td>
              <td className="py-0.5 pr-2 text-right tabular-nums">{r.event === "efficiency" ? "" : fmt(r.time, 3)}</td>
              <td className="py-0.5 text-right tabular-nums">
                {fmt(r.points, 1)} / {r.maxPoints}
              </td>
            </tr>
          ))}
          <tr className="border-t border-[color:var(--ss-border)] font-semibold">
            <td className="py-0.5 pr-2">Dynamic events</td>
            <td />
            <td className="py-0.5 text-right tabular-nums">
              {fmt(total, 1)} / {max}
            </td>
          </tr>
        </tbody>
      </table>
      <p className="mt-1 text-[10px] text-[color:var(--ss-text-dim)]">
        Estimates from the scoring formulas of FS Rules 2026 v1.1 (FSG) D 9, not official results. Each event uses the
        newest finished run of its case; set each case&apos;s reference values (the best teams&apos; time and energy).
        FSUK and FSAE score differently: check the current season&apos;s rules.
      </p>
    </section>
  );
}
