import { useState } from "react";
import { ChevronDown, ChevronRight, Download, FlaskConical, X } from "lucide-react";
import { confirmDialog } from "../../dialog";
import { useProjectStore } from "../../store/projectStore";
import type { Study, StudyPoint } from "../../types";

const NO_STUDIES: Study[] = [];

function statusText(p: StudyPoint): string {
  if (p.incomplete) return p.incomplete;
  if (p.status === "success") return "complete";
  if (p.status === "warning") return "complete, with warnings";
  return p.status; // failed, not run
}

/** The summary value a study's table opens on (the one Results' sweep view
 *  prefers too), or its first. */
function defaultKpi(study: Study): string {
  const labels = study.kpis.map((k) => k.label);
  return labels.find((l) => /consumption|final soc|fuel/i.test(l)) ?? labels[0] ?? "";
}

/** Download a study's whole table: a row per point with its value, status,
 *  run id and every summary value. */
function exportStudyCsv(study: Study) {
  const quote = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
  const header = [
    ...study.factors.map((f) => `${f.elementLabel} ${f.paramLabel}${f.unit ? ` [${f.unit}]` : ""}`),
    "status",
    "run id",
    ...study.kpis.map((k) => `${k.label}${k.unit ? ` [${k.unit}]` : ""}`),
  ];
  const rows = study.points.map((p) => [
    ...p.values.map(String),
    statusText(p),
    p.runId ?? "",
    ...study.kpis.map((k) => (k.label in p.kpis ? String(p.kpis[k.label]) : "")),
  ]);
  const csv = [header, ...rows].map((r) => r.map(quote).join(",")).join("\n");
  const url = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = `lightsim-study-${study.factors[0]?.paramKey ?? "sweep"}-${study.id}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

function StudyCard({ study, open, onToggle }: { study: Study; open: boolean; onToggle: () => void }) {
  const removeStudy = useProjectStore((s) => s.removeStudy);
  const [kpi, setKpi] = useState(() => defaultKpi(study));
  const factor = study.factors[0];
  const kpiUnit = study.kpis.find((k) => k.label === kpi)?.unit;
  const when = new Date(study.startedAt).toLocaleString();
  const complete = study.points.filter((p) => (p.status === "success" || p.status === "warning") && !p.incomplete);
  if (!factor) return null;

  return (
    <div className="mb-1.5 rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)]">
      <div className="flex items-center gap-1 px-1 py-0.5">
        <button
          className="flex min-w-0 flex-1 items-center gap-1 rounded px-0.5 py-0.5 text-left text-[11px] hover:bg-[color:var(--ss-hover)]"
          aria-expanded={open}
          title={`${factor.elementLabel} · ${factor.paramLabel} on '${study.caseName}', ${when}`}
          onClick={onToggle}
        >
          {open ? <ChevronDown size={12} className="shrink-0" /> : <ChevronRight size={12} className="shrink-0" />}
          <span className="min-w-0 flex-1">
            <span className="block truncate font-medium">
              {factor.elementLabel} · {factor.paramLabel}
            </span>
            <span className="block truncate text-[10px] text-[color:var(--ss-text-dim)]">
              {complete.length} of {study.points.length} complete · {when}
            </span>
          </span>
        </button>
        <button
          className="ss-toolbtn"
          title="Download this study's table (every result) as CSV"
          onClick={() => exportStudyCsv(study)}
        >
          <Download size={12} />
        </button>
        <button
          className="ss-toolbtn"
          title="Delete this study (its runs stay in the Results history)"
          onClick={() =>
            void confirmDialog({
              title: "Delete this study?",
              message: `This removes the ${factor.paramLabel} study of ${when} and its table from the project.`,
              confirmLabel: "Delete study",
              danger: true,
            }).then((ok) => ok && removeStudy(study.id))
          }
        >
          <X size={12} />
        </button>
      </div>
      {open && (
        <div className="border-t border-[color:var(--ss-border)] px-1.5 py-1">
          <div className="mb-1 text-[10px] text-[color:var(--ss-text-dim)]">
            Case '{study.caseName}' · {factor.values.length} value(s)
          </div>
          {study.kpis.length > 0 && (
            <label className="mb-1 flex items-center gap-1 text-[11px] text-[color:var(--ss-text-dim)]">
              Result
              <select className="ss-input min-w-0 flex-1" value={kpi} onChange={(e) => setKpi(e.target.value)}>
                {study.kpis.map((k) => (
                  <option key={k.label} value={k.label}>
                    {k.label}
                    {k.unit ? ` (${k.unit})` : ""}
                  </option>
                ))}
              </select>
            </label>
          )}
          <table className="w-full border-collapse" aria-label={`Results of the ${factor.paramLabel} study of ${when}`}>
            <thead>
              <tr>
                <th className="ss-th text-right">
                  {factor.paramLabel}
                  {factor.unit ? ` [${factor.unit}]` : ""}
                </th>
                <th className="ss-th text-right">{kpiUnit ? `[${kpiUnit}]` : "Value"}</th>
                <th className="ss-th">Status</th>
              </tr>
            </thead>
            <tbody>
              {study.points.map((p, i) => {
                const v = p.kpis[kpi];
                const notValid = p.notValid?.[kpi];
                return (
                  <tr key={i} title={p.runId ? `run ${p.runId}` : undefined}>
                    <td className="ss-td text-right font-mono">{p.values.join(", ")}</td>
                    <td
                      className={`ss-td text-right font-mono ${notValid ? "text-amber-600" : ""}`}
                      title={notValid ? `Not valid: ${notValid}` : undefined}
                    >
                      {typeof v === "number" ? v.toLocaleString(undefined, { maximumFractionDigits: 4 }) : "—"}
                    </td>
                    <td
                      className={`ss-td ${p.status === "success" && !p.incomplete ? "" : "text-[color:var(--ss-text-dim)]"}`}
                    >
                      {statusText(p)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

/** Studies saved with the project (STU-03), newest first and the newest
 *  opened: each sweep's definition and its results table, a row per point.
 *  Nothing is shown before the first study. */
export function StudiesList() {
  const studies = useProjectStore((s) => s.project?.studies ?? NO_STUDIES);
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  if (studies.length === 0) return null;
  const newest = studies[studies.length - 1].id;

  return (
    <section aria-label="Saved studies" className="mt-4">
      <div className="mb-1 flex items-center gap-1 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
        <FlaskConical size={12} /> Saved studies ({studies.length})
      </div>
      <p className="mb-2 text-[11px] text-[color:var(--ss-text-dim)]">
        Each sweep is kept with the project with its table of results, also after its runs leave the
        Results history. Save the project to keep new studies on disk.
      </p>
      {[...studies].reverse().map((st) => {
        const open = toggled[st.id] ?? st.id === newest;
        return (
          <StudyCard
            key={st.id}
            study={st}
            open={open}
            onToggle={() => setToggled((t) => ({ ...t, [st.id]: !open }))}
          />
        );
      })}
    </section>
  );
}
