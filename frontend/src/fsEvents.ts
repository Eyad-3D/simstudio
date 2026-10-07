// Formula Student dynamic events (MOD-43): the engine scores each event's
// run (fs_events.py, FS Rules 2026 v1.1 (FSG) D 9); this gathers the points
// of the newest run of each event's case into one table.
import type { FsEvent, Project, SimRun } from "./types";

export type { FsEvent };

export const FS_EVENTS: FsEvent[] = ["acceleration", "skidpad", "autocross", "endurance"];

export const FS_EVENT_NAMES: Record<FsEvent | "efficiency", string> = {
  acceleration: "Acceleration",
  skidpad: "Skidpad",
  autocross: "Autocross",
  endurance: "Endurance",
  efficiency: "Efficiency",
};

/** Maximum points, FS Rules 2026 v1.1 (FSG) table 3. */
export const FS_MAX_POINTS: Record<FsEvent | "efficiency", number> = {
  acceleration: 50,
  skidpad: 50,
  autocross: 100,
  endurance: 250,
  efficiency: 75,
};

export interface FsPointsRow {
  event: FsEvent | "efficiency";
  caseId?: string;
  caseName?: string;
  /** the event's time as the rules take it, s */
  time?: number;
  points?: number;
  maxPoints: number;
  /** why there are no points, or why they are not valid */
  note?: string;
  /** a rule check the run failed */
  breach?: string;
}

const value = (run: SimRun, test: (label: string) => boolean) => run.result.summary.find((s) => test(s.label));

/** One row per event (and efficiency) from the newest finished run of the
 *  case marked for it; a total of the points there are. */
export function fsPointsTable(project: Project, runs: SimRun[]): { rows: FsPointsRow[]; total?: number } {
  const rows: FsPointsRow[] = [];
  for (const ev of FS_EVENTS) {
    const c = project.cases.find((x) => x.fsEvent === ev);
    const row: FsPointsRow = {
      event: ev,
      caseId: c?.id,
      caseName: c?.name,
      maxPoints: FS_MAX_POINTS[ev],
    };
    rows.push(row);
    const eff: FsPointsRow = {
      event: "efficiency",
      caseId: c?.id,
      caseName: c?.name,
      maxPoints: FS_MAX_POINTS.efficiency,
    };
    if (ev === "endurance") rows.push(eff);
    if (!c) {
      row.note = "No case is marked for this event.";
      continue;
    }
    const run = runs.find((r) => r.caseId === c.id && !r.sweepId && r.status !== "running" && !r.incomplete);
    if (!run) {
      row.note = "Not run yet.";
      continue;
    }
    if (run.status === "failed") {
      row.note = "The run failed: see Messages.";
      continue;
    }
    const name = FS_EVENT_NAMES[ev];
    row.time = value(run, (l) => l.startsWith(`${name} time (`))?.value;
    const pts = value(run, (l) => l === `${name} points (estimate)`);
    row.points = pts?.value;
    const failed = run.result.summary.find((s) => s.label.startsWith("Rule check:") && s.passed === false);
    if (failed)
      row.breach = `${failed.label.replace("Rule check: ", "")}: ${failed.value} ${failed.unit} over ${failed.limit} ${failed.unit}`;
    if (pts?.notValid) row.note = `Not valid: ${pts.notValid}`;
    else if (pts == null) row.note = "Set the case's Reference time (the fastest team's time).";
    if (ev === "endurance") {
      const effPts = value(run, (l) => l === "Efficiency points (estimate)");
      eff.points = effPts?.value;
      eff.breach = row.breach;
      if (effPts?.notValid) eff.note = `Not valid: ${effPts.notValid}`;
      else if (effPts == null) eff.note = "Set the case's Reference energy (the most efficient team's).";
    }
  }
  const scored = rows.filter((r) => r.points != null);
  return {
    rows,
    total: scored.length ? scored.reduce((a, r) => a + (r.points ?? 0), 0) : undefined,
  };
}
