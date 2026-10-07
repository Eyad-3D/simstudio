// Expected values (VAL-35): grade a value against a number the user trusts.
// The engine grades each run (backend/app/solver/references.py); this copy
// grades a study's points from their stored values, the same way.
import type { ReferenceCheck, ReferenceGrade, ReferenceValue, SummaryValue } from "./types";

/** Green within the tolerance, amber within twice it, red beyond. */
export function gradeGap(diff: number, tol: number): Exclude<ReferenceGrade, "missing" | "not valid"> {
  const d = Math.abs(diff);
  if (d <= tol * (1 + 1e-9)) return "within";
  return d <= 2 * tol * (1 + 1e-9) ? "near" : "outside";
}

/** The tolerance in the value's unit. */
export function toleranceOf(ref: ReferenceValue): number {
  return Math.abs(ref.tolerance) * (ref.tolerancePct === false ? 1 : Math.abs(ref.value) / 100);
}

/** A reference against a value (a summary row, or a study point's KPI). */
export function checkReference(
  ref: ReferenceValue,
  value: number | undefined,
  unit = "",
  notValid?: string | null,
): ReferenceCheck {
  const tol = toleranceOf(ref);
  const base = { label: ref.kpi, reference: ref.value, unit, tolerance: tol, source: ref.source ?? "", automatic: false, bound: "two-sided" as const };
  if (value === undefined) return { ...base, grade: "missing", note: "this run has no summary value of that name" };
  const diff = value - ref.value;
  return {
    ...base,
    value,
    difference: diff,
    differencePct: ref.value ? (100 * diff) / ref.value : null,
    grade: notValid ? "not valid" : gradeGap(diff, tol),
    note: notValid ? `not valid: ${notValid}` : null,
  };
}

export function checkReferences(refs: ReferenceValue[], summary: SummaryValue[]): ReferenceCheck[] {
  return refs.map((r) => {
    const row = summary.find((s) => s.label === r.kpi);
    return checkReference(r, row?.value, row?.unit, row?.notValid);
  });
}

export const GRADE_TEXT: Record<ReferenceGrade, string> = {
  within: "within",
  near: "near",
  outside: "outside",
  missing: "no value",
  "not valid": "not valid",
};

/** "−0.20 s (−2.7 %)", the gap a chip shows. */
export function gapText(c: ReferenceCheck): string {
  if (c.difference == null) return "—";
  const fmt = (x: number) => (x > 0 ? "+" : x < 0 ? "−" : "±") + Math.abs(x).toLocaleString(undefined, { maximumSignificantDigits: 3 });
  const pct = c.differencePct == null ? "" : ` (${fmt(c.differencePct)} %)`;
  return `${fmt(c.difference)}${c.unit ? ` ${c.unit}` : ""}${pct}`;
}

/** What the grade means, for a tooltip. */
export function gradeTitle(c: ReferenceCheck): string {
  const tol = c.tolerance.toLocaleString(undefined, { maximumSignificantDigits: 3 });
  const what =
    c.bound === "at most"
      ? `must be at most ${c.reference.toLocaleString()} ${c.unit} (+${tol})`
      : c.bound === "at least"
        ? `must be at least ${c.reference.toLocaleString()} ${c.unit} (−${tol})`
        : `expected ${c.reference.toLocaleString()} ${c.unit} ± ${tol}: green within that, amber within twice it, red beyond`;
  return [`${c.label}: ${what}`, c.source && `Source: ${c.source}`, c.note].filter(Boolean).join("\n");
}
