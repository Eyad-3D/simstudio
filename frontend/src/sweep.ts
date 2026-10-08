// A parameter sweep's values and progress (STU-17): the values the sweep form
// runs (evenly spaced, log spaced or a typed list) and how long a running
// sweep has left.
import type { StudyPoint } from "./types";

/** The most points a sweep runs. Each point is a run stored with the project,
 *  so this guards the disk budget for runs; the engine itself takes up to
 *  2,000 points in one study request. */
export const MAX_SWEEP_POINTS = 200;

export type SweepSpacing = "linear" | "log" | "list";

/** The values a sweep form gives, and why it cannot run them (null: it can).
 *  `note` says what was left out of a typed list. */
export interface SweepValues {
  values: number[];
  problem: string | null;
  note?: string;
}

/** Twelve significant digits: 0.1 + 0.2 is 0.3, and no swept value is
 *  rounded away however small it is. */
const clean = (v: number) => +v.toPrecision(12);
/** Six significant digits for log-spaced values (31.6228, not 31.6227766017). */
const sig6 = (v: number) => +v.toPrecision(6);

const steps = (n: number) => Math.max(1, Math.min(MAX_SWEEP_POINTS, Math.round(n) || 1));

/** `n` values from `from` to `to`, evenly spaced (both ends included). */
export function linearValues(from: number, to: number, n: number): number[] {
  const count = steps(n);
  if (count === 1) return [clean(from)];
  return Array.from({ length: count }, (_, i) => clean(i === count - 1 ? to : from + ((to - from) * i) / (count - 1)));
}

/** `n` values from `from` to `to`, each the same factor from the one before
 *  (evenly spaced on a log axis); both ends must be above 0. */
export function logValues(from: number, to: number, n: number): SweepValues {
  if (!(from > 0) || !(to > 0)) return { values: [], problem: "Log steps need both ends above 0." };
  const count = steps(n);
  if (count === 1) return { values: [from], problem: null };
  const ratio = Math.log(to / from) / (count - 1);
  const values = Array.from({ length: count }, (_, i) =>
    i === 0 ? from : i === count - 1 ? to : sig6(from * Math.exp(ratio * i)),
  );
  return dedupe(values);
}

/** The values typed in a list: separated by commas, semicolons, spaces or
 *  new lines, with a point for decimals ("1200, 1350, 1.5e3"). They run in
 *  the order typed; a repeated value runs once. */
export function parseValueList(text: string): SweepValues {
  const tokens = text.split(/[\s,;]+/).filter(Boolean);
  if (tokens.length === 0) return { values: [], problem: "Type the values to run, for example 1200, 1350, 1500." };
  const bad = tokens.find((t) => !Number.isFinite(Number(t)));
  if (bad !== undefined) return { values: [], problem: `'${bad}' is not a number: use a point for decimals.` };
  const out = dedupe(tokens.map(Number));
  if (out.values.length > MAX_SWEEP_POINTS)
    return { values: [], problem: `At most ${MAX_SWEEP_POINTS} values: the list has ${out.values.length}.` };
  return out;
}

function dedupe(values: number[]): SweepValues {
  const unique = [...new Set(values)];
  const left = values.length - unique.length;
  return {
    values: unique,
    problem: null,
    ...(left > 0 ? { note: `${left} repeated value${left > 1 ? "s" : ""} left out.` } : {}),
  };
}

/** The values of a sweep form. */
export function sweepValues(
  spacing: SweepSpacing,
  range: { from: number; to: number; steps: number },
  list: string,
): SweepValues {
  if (spacing === "list") return parseValueList(list);
  if (spacing === "log") return logValues(range.from, range.to, range.steps);
  return dedupe(linearValues(range.from, range.to, range.steps));
}

/** Whether values (in any order) are evenly spaced on a log axis over at
 *  least one decade, so that their chart reads best with a log x axis. */
export function looksLogSpaced(values: number[]): boolean {
  const xs = [...new Set(values)].sort((a, b) => a - b);
  if (xs.length < 3 || xs[0] <= 0 || xs[xs.length - 1] / xs[0] < 10) return false;
  const step = Math.log(xs[1] / xs[0]);
  return xs.every((x, i) => i === 0 || Math.abs(Math.log(x / xs[i - 1]) - step) <= 1e-3 * Math.abs(step));
}

/** A running sweep: how many of its points have ended, and when (epoch ms)
 *  it started and its latest point ended. */
export interface SweepProgress {
  total: number;
  done: number;
  startedAt: number;
  lastAt: number | null;
}

/** About how many seconds a sweep has left at `now`, from the pace of the
 *  points that have ended (they run side by side, so this is the pool's
 *  pace, not one run's); null until a point has ended. */
export function timeLeft(p: SweepProgress, now: number): number | null {
  if (p.done >= p.total) return 0;
  if (p.done === 0 || p.lastAt == null) return null;
  const end = p.startedAt + ((p.lastAt - p.startedAt) / p.done) * p.total;
  return Math.max(0, (end - now) / 1000);
}

/** "about 40 s left", "about 3 min left", "about 1 h 20 min left". */
export function timeLeftText(s: number | null): string {
  if (s == null) return "working out the time left";
  if (s < 5) return "almost done";
  if (s < 57.5) return `about ${Math.round(s / 5) * 5} s left`;
  const min = Math.round(s / 60);
  if (min < 60) return `about ${min} min left`;
  const h = Math.floor(min / 60);
  return `about ${h} h${min % 60 ? ` ${min % 60} min` : ""} left`;
}

/** "3 of 10 points done · about 40 s left" */
export function progressText(p: SweepProgress, now: number): string {
  return `${p.done} of ${p.total} point${p.total === 1 ? "" : "s"} done · ${timeLeftText(timeLeft(p, now))}`;
}

/** A study point whose run ended normally: not stopped, failed or not run. */
export const pointComplete = (p: StudyPoint) => (p.status === "success" || p.status === "warning") && !p.incomplete;
