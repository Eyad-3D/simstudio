// What the Study view (STU-16) draws: a saved study's results, a chart per
// figure against the swept value, read from the study kept with the
// project's runs (PLT-34), not from the runs in the Results history.
import type { Study } from "../../types";
import { looksLogSpaced, pointComplete } from "../../sweep";
import { headlineRows } from "./chartUtils";

/** The most charts the Study view draws at once. */
export const MAX_FIGURES = 12;

/** The study the Study view shows: the one picked while it is kept, else
 *  the study of the run shown (a sweep point's), else the newest of the case
 *  shown, else the newest (studies are oldest first). */
export function studyToShow(
  studies: Study[],
  picked?: string,
  runSweepId?: string,
  caseId?: string,
): Study | undefined {
  const by = (id?: string) => (id ? studies.find((s) => s.id === id) : undefined);
  const newest = [...studies].reverse();
  return by(picked) ?? by(runSweepId) ?? newest.find((s) => s.caseId === caseId) ?? newest[0];
}

/** The figures a study's charts show: those picked that the study has (in
 *  the order picked), else its headline numbers, else its first (at most 4). */
export function figuresOf(study: Study, picked?: string[]): string[] {
  const labels = new Set(study.kpis.map((k) => k.label));
  const kept = (picked ?? []).filter((l) => labels.has(l));
  if (kept.length) return kept.slice(0, MAX_FIGURES);
  return headlineRows(study.kpis)
    .slice(0, 4)
    .map((k) => k.label);
}

export interface StudyChartData {
  /** the first factor's values with a complete point, ascending */
  x: number[];
  /** a line for each value of a second factor (one line for one factor):
   *  its values, and apart the values its run's checks rule not valid */
  lines: { label: string; y: (number | null)[]; notValid: (number | null)[] }[];
  /** how many points are left out: their run stopped, failed or did not run */
  left: number;
  /** how many values are not valid (drawn hollow) */
  notValid: number;
  /** whether the swept values are log spaced: a log x axis */
  logX: boolean;
}

/** One figure of a study against its swept value. */
export function chartData(study: Study, figure: string): StudyChartData {
  const second = study.factors[1];
  const done = study.points.filter(pointComplete);
  const x = [...new Set(done.map((p) => p.values[0]))].sort((a, b) => a - b);
  const groups = second ? [...new Set(study.points.map((p) => p.values[1]))].sort((a, b) => a - b) : [undefined];
  let notValid = 0;
  const lines = groups.map((g) => {
    const y: (number | null)[] = x.map(() => null);
    const bad: (number | null)[] = x.map(() => null);
    for (const p of done) {
      const v = p.kpis[figure];
      if ((g !== undefined && p.values[1] !== g) || typeof v !== "number") continue;
      const i = x.indexOf(p.values[0]);
      if (p.notValid?.[figure]) {
        bad[i] = v;
        notValid++;
      } else y[i] = v;
    }
    const label = second ? `${second.paramLabel} ${g}${second.unit ? ` ${second.unit}` : ""}` : figure;
    return { label, y, notValid: bad };
  });
  return {
    x,
    lines,
    left: study.points.length - done.length,
    notValid,
    logX: looksLogSpaced(study.factors[0]?.values ?? []),
  };
}
