// The table of runs in Results: a row per stored run, its main figures and
// how the rows sort.
import { sameFigure, type SimRun, type StoredRunInfo, type SummaryValue } from "../../types";
import { HEADLINE } from "./chartUtils";

/** A run as the table lists it: one in memory, or one only listed on disk
 *  (its channel data is read when it is ticked or shown). */
export interface RunRow {
  id: string;
  caseName: string;
  startedAt: number;
  /** "running" while it runs */
  status: SimRun["status"];
  incomplete?: string;
  name?: string;
  sweepParam?: string;
  sweepValue?: number;
  sweepUnit?: string;
  summary: SummaryValue[];
  /** its channel data is in memory */
  loaded: boolean;
}

/** The rows: every run in memory, then every stored run that is not, newest first. */
export function runRows(runs: SimRun[], index: StoredRunInfo[]): RunRow[] {
  const inMemory = new Set(runs.map((r) => r.id));
  const row = (r: SimRun | StoredRunInfo, summary: SummaryValue[], loaded: boolean): RunRow => ({
    id: r.id,
    caseName: r.caseName,
    startedAt: r.startedAt,
    status: r.status,
    incomplete: r.incomplete,
    name: r.name,
    sweepParam: r.sweepParam,
    sweepValue: r.sweepValue,
    sweepUnit: r.sweepUnit,
    summary,
    loaded,
  });
  return [
    ...runs.map((r) => row(r, r.result.summary, true)),
    ...index.filter((e) => !inMemory.has(e.id)).map((e) => row(e, e.summary, false)),
  ].sort((a, b) => b.startedAt - a.startedAt);
}

/** A figure column: the summary row it reads (matched by key, else label). */
export type FigureColumn = Pick<SummaryValue, "key" | "label" | "unit">;

/** At most `max` figure columns: the headline figures (consumption,
 *  distance, final SOC, energy …) the runs have, in the headline order. */
export function figureColumns(rows: RunRow[], max = 4): FigureColumn[] {
  const cols: FigureColumn[] = [];
  for (const re of HEADLINE)
    for (const r of rows)
      for (const s of r.summary)
        if (re.test(s.label) && !cols.some((c) => sameFigure(c, s))) cols.push({ key: s.key, label: s.label, unit: s.unit });
  return cols.slice(0, max);
}

export const figureOf = (r: RunRow, c: FigureColumn) => r.summary.find((s) => sameFigure(s, c));

/** The column the table is sorted by: a fixed one, or a figure column's index. */
export type SortKey = "ran" | "name" | "case" | "status" | number;
export type Sort = { key: SortKey; desc: boolean };

/** The outcome as the table writes it. */
export const statusText = (r: RunRow) =>
  r.status === "running" ? "running" : r.incomplete ? "incomplete" : r.status === "success" ? "success" : r.status;

/** Every status cell text, and the colour it is drawn in; contrast.test.ts
 *  checks each on the table's rows (plain, hovered, shown). */
export const STATUS_COLOR: Record<string, string> = {
  success: "var(--ss-text-dim)",
  "reading…": "var(--ss-text-dim)",
  running: "var(--ss-text-dim)",
  incomplete: "var(--ss-warn)",
  warning: "var(--ss-warn)",
  failed: "var(--ss-err)",
  error: "var(--ss-err)",
};
/** The shown row's background, checked with the status colours. */
export const SHOWN_ROW = "var(--ss-accent-soft)";

/** A sweep point's value, else the run's own name. */
export const rowName = (r: RunRow) =>
  r.sweepValue != null ? `${r.sweepParam ?? "value"} ${r.sweepValue}${r.sweepUnit ? ` ${r.sweepUnit}` : ""}` : (r.name ?? "");

/** The rows in the order of `sort`; rows without the value (no figure, no
 *  name) last either way, then newest first. */
export function sortRows(rows: RunRow[], sort: Sort, cols: FigureColumn[]): RunRow[] {
  const value = (r: RunRow): string | number | undefined => {
    switch (sort.key) {
      case "ran":
        return r.startedAt;
      case "name":
        return rowName(r) || undefined;
      case "case":
        return r.caseName;
      case "status":
        return statusText(r);
      default:
        return cols[sort.key] ? figureOf(r, cols[sort.key])?.value : undefined;
    }
  };
  const keyed = rows.map((r) => ({ r, v: value(r) }));
  return keyed
    .sort((a, b) => {
      if (a.v === undefined || b.v === undefined)
        return a.v === b.v ? b.r.startedAt - a.r.startedAt : a.v === undefined ? 1 : -1;
      const c =
        typeof a.v === "number" && typeof b.v === "number"
          ? a.v - b.v
          : String(a.v).localeCompare(String(b.v), undefined, { numeric: true });
      return (sort.desc ? -c : c) || b.r.startedAt - a.r.startedAt;
    })
    .map((k) => k.r);
}

/** When a run started: its clock time today, with the day before that. */
export function ranText(startedAt: number, now = Date.now()): string {
  const d = new Date(startedAt);
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  return d.toDateString() === new Date(now).toDateString()
    ? time
    : `${d.toLocaleDateString([], { day: "numeric", month: "short" })} ${time}`;
}
