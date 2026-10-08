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
    // (an endurance from an imported trace, a Cycle case, has no points)
    const c = project.cases.find((x) => x.fsEvent === ev && (x.kind ?? "cycle") !== "cycle");
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

// ---- the endurance energy study's pack axis (STU-38) ----------------------

type Params = Record<string, unknown>;

const num = (v: unknown, fallback = 0): number => {
  const n = Number(v);
  return Number.isFinite(n) ? n : fallback;
};

/** A 1D table's mean over 0 to 100 (its ends held), by trapezoids, as the
 *  engine's ocv_mean reads an open-circuit voltage table; NaN when it is
 *  not a table. */
export function tableMean(raw: unknown): number {
  if (!raw || typeof raw !== "object") return NaN;
  const pts = Object.entries(raw as Record<string, unknown>)
    .map(([x, y]) => [Number(x), Number(y)] as const)
    .filter(([x, y]) => Number.isFinite(x) && Number.isFinite(y))
    .sort((a, b) => a[0] - b[0]);
  if (!pts.length) return NaN;
  const at = (x: number) => {
    if (x <= pts[0][0]) return pts[0][1];
    for (let i = 1; i < pts.length; i++) {
      const [x1, y1] = pts[i];
      if (x <= x1) {
        const [x0, y0] = pts[i - 1];
        return y0 + ((y1 - y0) * (x - x0)) / (x1 - x0);
      }
    }
    return pts[pts.length - 1][1];
  };
  const xs = [0, ...pts.map(([x]) => x).filter((x) => x > 0 && x < 100), 100];
  let area = 0;
  for (let i = 1; i < xs.length; i++) area += ((xs[i] - xs[i - 1]) * (at(xs[i - 1]) + at(xs[i]))) / 2;
  return area / 100;
}

/** A battery's energy, kWh, as the study's pack axis reads it, from its
 *  parameters (library defaults, the part's and the case's values): its
 *  Usable Capacity, or built from cells (MOD-08), cells in series × in
 *  parallel × the cell's charge × its mean open-circuit voltage. */
export function packKwh(p: Params): number {
  if (p.pack_model === "Cells") {
    const kwh =
      (num(p.series_cells, 96) * num(p.parallel_cells, 30) * num(p.cell_capacity_Ah, 5) * tableMean(p.cell_ocv_table)) /
      1000;
    return Number.isFinite(kwh) ? kwh : NaN;
  }
  return num(p.capacity_kWh, NaN);
}

/** The battery values that make a pack of ``kwh``. Pack values: the Usable
 *  Capacity, and the Charge Capacity scaled with it, since the engine reads
 *  the charge from the amp-hours when they are set (a study that changed
 *  only the kWh ran one pack size); with no base to scale from, the
 *  amp-hours follow the kWh (0). Cells: the cell's charge, scaled. */
export function packOverrides(p: Params, kwh: number): Record<string, number> {
  const base = packKwh(p);
  const scale = Number.isFinite(base) && base > 0 ? kwh / base : NaN;
  if (p.pack_model === "Cells")
    return Number.isFinite(scale) ? { cell_capacity_Ah: num(p.cell_capacity_Ah, 5) * scale } : {};
  const ah = num(p.capacity_Ah);
  if (ah <= 0) return { capacity_kWh: kwh };
  return { capacity_kWh: kwh, capacity_Ah: Number.isFinite(scale) ? ah * scale : 0 };
}
