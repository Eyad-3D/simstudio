// The Study view (STU-16): a grid of small charts, each a result of a saved
// study against the swept value. It reads the study kept with the project's
// runs (PLT-34), so it works after the project is opened again and after the
// study's runs have left the Results history.
import { useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import { Download, ListChecks } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import type { Study } from "../../types";
import { pointComplete } from "../../sweep";
import { useDismiss } from "../useDismiss";
import { PALETTE, fmtNum, unitAxis, useHasSize, yRange } from "./chartUtils";
import { Plot, axisStyle, type PlotOptions } from "./Plot";
import { exportStudyCsv } from "./StudiesList";
import { MAX_FIGURES, chartData, figuresOf, studyToShow } from "./studyCharts";

const NO_STUDIES: Study[] = [];

/** A study's name in the picker: what was swept, on which case, when. */
const studyName = (s: Study) =>
  `${s.factors.map((f) => `${f.elementLabel} · ${f.paramLabel}`).join(" × ")} on '${s.caseName}' · ${new Date(s.startedAt).toLocaleString()}`;

/** One figure against the swept value: a line through the complete points
 *  (a line per value of a second factor), values that are not valid hollow. */
function StudyChart({ study, figure }: { study: Study; figure: string }) {
  const theme = useUIStore((s) => s.theme);
  const { ref, hasSize } = useHasSize<HTMLDivElement>();
  const data = useMemo(() => chartData(study, figure), [study, figure]);
  const unit = study.kpis.find((k) => k.label === figure)?.unit ?? "";
  const factor = study.factors[0];
  // a line's hollow markers, when it has values that are not valid
  const hollow = useMemo(() => data.lines.map((l) => l.notValid.some((v) => v != null)), [data]);
  const cols = useMemo(
    (): uPlot.AlignedData => [data.x, ...data.lines.flatMap((l, i) => (hollow[i] ? [l.y, l.notValid] : [l.y]))],
    [data, hollow],
  );
  const options = useMemo((): PlotOptions => {
    const axis = axisStyle(theme);
    const bg = getComputedStyle(document.documentElement).getPropertyValue("--ss-panel").trim();
    const xUnit = factor?.unit ? ` ${factor.unit}` : "";
    const val = (_u: uPlot, v: number | null) => (v == null ? "—" : `${fmtNum(v)}${unit ? ` ${unit}` : ""}`);
    return {
      scales: {
        x: data.logX ? { time: false, distr: 3, log: 10 } : { time: false },
        y: { range: yRange({}, uPlot.rangeNum) },
      },
      series: [
        // the swept value as the study's table shows it, unrounded
        { label: factor?.paramLabel ?? "value", value: (_u, v) => (v == null ? "—" : `${v}${xUnit}`) },
        ...data.lines.flatMap((l, i) => {
          const color = PALETTE[i % PALETTE.length];
          const line: uPlot.Series = {
            label: l.label,
            stroke: color,
            width: 1.8,
            spanGaps: true,
            points: { show: true, size: 6, fill: color },
            value: val,
          };
          const notValid: uPlot.Series = {
            label: data.lines.length > 1 ? `${l.label} (not valid)` : "not valid",
            stroke: color,
            paths: () => null,
            points: { show: true, size: 8, width: 1.5, fill: bg },
            value: val,
          };
          return hollow[i] ? [line, notValid] : [line];
        }),
      ],
      axes: [
        { ...axis, label: `${factor?.paramLabel ?? "value"}${factor?.unit ? ` [${factor.unit}]` : ""}`, size: 24 },
        { ...axis, ...unitAxis(1), label: unit || figure, size: 54 },
      ],
      cursor: { drag: { x: true, y: false } },
    };
  }, [data, hollow, factor, unit, figure, theme]);
  const hasValues = data.lines.some((l) => l.y.some((v) => v != null) || l.notValid.some((v) => v != null));
  return (
    <figure className="m-0 flex min-w-0 flex-col rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)]">
      <figcaption className="truncate px-1.5 pt-1 text-[11px] font-semibold" title={figure}>
        {figure}
        {unit && <span className="font-normal text-[color:var(--ss-text-dim)]"> [{unit}]</span>}
        {data.notValid > 0 && (
          <span className="font-normal text-[color:var(--ss-warn)]" title="Hollow: the run's checks rule these values not valid">
            {" "}
            · {data.notValid} not valid
          </span>
        )}
      </figcaption>
      <div className="h-[210px] p-1" ref={ref}>
        {hasValues && hasSize ? (
          <Plot options={options} data={cols} label="Study chart" />
        ) : (
          <div className="flex h-full items-center justify-center text-[11px] text-[color:var(--ss-text-dim)]">
            {hasValues ? "" : "No complete point has this result."}
          </div>
        )}
      </div>
    </figure>
  );
}

/** The figures to chart: a tick for each of the study's results. */
function FiguresMenu({ study, figures, onChange }: { study: Study; figures: string[]; onChange: (f: string[]) => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  useDismiss(open, () => setOpen(false), ref, button);
  const full = figures.length >= MAX_FIGURES;
  return (
    <div className="relative" ref={ref}>
      <button
        ref={button}
        className="ss-toolbtn border border-[color:var(--ss-border)]"
        aria-expanded={open}
        title={`The results to chart against the swept value (at most ${MAX_FIGURES})`}
        onClick={() => setOpen(!open)}
      >
        <ListChecks size={12} /> Figures ({figures.length})
      </button>
      {open && (
        <div
          role="group"
          aria-label="Figures to chart"
          className="absolute left-0 top-full z-50 mt-1 flex max-h-[300px] w-[300px] flex-col overflow-y-auto rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] p-1 text-[11px] shadow-lg"
        >
          {study.kpis.map((k) => {
            const on = figures.includes(k.label);
            return (
              <label key={k.label} className="flex items-center gap-1.5 rounded px-1 py-0.5 hover:bg-[color:var(--ss-hover)]">
                <input
                  type="checkbox"
                  checked={on}
                  // (the last chart stays; at most MAX_FIGURES)
                  disabled={on ? figures.length === 1 : full}
                  onChange={() => onChange(on ? figures.filter((f) => f !== k.label) : [...figures, k.label])}
                />
                <span className="truncate" title={k.label}>
                  {k.label}
                  {k.unit ? ` (${k.unit})` : ""}
                </span>
              </label>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** The Study view: pick a saved study and its results; a chart each.
 *  `runSweepId` and `caseId` are the run Results shows, whose study (or
 *  case's newest study) it opens on. */
export function StudyView({ runSweepId, caseId }: { runSweepId?: string; caseId?: string }) {
  const studies = useProjectStore((s) => s.studies ?? NO_STUDIES);
  const projectId = useProjectStore((s) => s.project?.id ?? "");
  const choice = useUIStore((s) => s.resultsViews[projectId]);
  const study = studyToShow(studies, choice?.study, runSweepId, caseId);
  if (!study) {
    return (
      <div className="flex h-full items-center justify-center px-4 text-center text-[12px] text-[color:var(--ss-text-dim)]">
        No saved studies yet: run a parameter sweep from Cases &amp; Parameters.
      </div>
    );
  }
  const figures = figuresOf(study, choice?.studyFigures);
  const save = (patch: { study?: string; studyFigures?: string[] }) =>
    projectId && useUIStore.getState().setStudyView(projectId, patch);
  const complete = study.points.filter(pointComplete).length;
  const left = study.points.length - complete;
  return (
    <section aria-label="Study charts" className="flex min-h-0 flex-col gap-1.5">
      <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-[color:var(--ss-text-dim)]">
        <select
          className="ss-input min-w-0 max-w-[420px] flex-1 py-0.5 text-[11px]"
          aria-label="Study"
          title="A saved study (Cases & Parameters → Saved studies)"
          value={study.id}
          onChange={(e) => save({ study: e.target.value })}
        >
          {[...studies].reverse().map((s) => (
            <option key={s.id} value={s.id}>
              {studyName(s)}
            </option>
          ))}
        </select>
        <FiguresMenu study={study} figures={figures} onChange={(f) => save({ studyFigures: f })} />
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)]"
          title="Download this study's table (every result) as CSV"
          onClick={() => exportStudyCsv(study)}
        >
          <Download size={12} /> Table CSV
        </button>
      </div>
      <div className="text-[11px] text-[color:var(--ss-text-dim)]">
        {complete} of {study.points.length} point{study.points.length === 1 ? "" : "s"} complete
        {left > 0 && <span className="text-[color:var(--ss-warn)]"> · {left} left out (stopped, failed or not run)</span>}
        {" · "}from the saved study, kept with the project&apos;s runs
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-1.5">
        {figures.map((f) => (
          <StudyChart key={`${study.id}:${f}`} study={study} figure={f} />
        ))}
      </div>
    </section>
  );
}
