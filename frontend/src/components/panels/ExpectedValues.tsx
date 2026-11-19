import { useId } from "react";
import { Plus, Trash2 } from "lucide-react";
import { gapText, GRADE_TEXT, gradeTitle } from "../../references";
import { useProjectStore } from "../../store/projectStore";
import type { ReferenceCheck, ReferenceGrade, ReferenceValue, SimCase } from "../../types";

const GRADE_CLASS: Record<ReferenceGrade, string> = {
  within: "border-[color:var(--ss-ok)] text-[color:var(--ss-ok)]",
  near: "border-[color:var(--ss-warn)] text-[color:var(--ss-warn)]",
  outside: "border-[color:var(--ss-err)] text-[color:var(--ss-err)]",
  missing: "border-[color:var(--ss-border)] text-[color:var(--ss-text-dim)]",
  "not valid": "border-[color:var(--ss-warn)] text-[color:var(--ss-warn)]",
};

/** A grade as a small coloured chip: green within the tolerance, amber
 *  within twice it, red beyond (the word says it too, not only the colour). */
export function GradeChip({ check }: { check: ReferenceCheck }) {
  return (
    <span
      className={`rounded border px-1 font-sans text-[10px] font-semibold ${GRADE_CLASS[check.grade]}`}
      title={gradeTitle(check)}
    >
      {check.automatic && check.grade === "within" ? "ok" : GRADE_TEXT[check.grade]}
    </span>
  );
}

/** One check in a line: its value against the reference and the gap. */
function CheckLine({ c }: { c: ReferenceCheck }) {
  const num = (x: number | null | undefined) =>
    x == null ? "—" : x.toLocaleString(undefined, { maximumSignificantDigits: 4 });
  const rel = c.bound === "at most" ? "≤" : c.bound === "at least" ? "≥" : "vs";
  return (
    <li className="flex min-w-0 flex-wrap items-baseline gap-x-1.5" title={gradeTitle(c)}>
      <GradeChip check={c} />
      <span className="min-w-0 break-words">{c.label}</span>
      <span className="font-mono">
        {num(c.value)} {rel} {num(c.reference)} {c.unit}
      </span>
      {c.bound === "two-sided" && <span className="font-mono text-[color:var(--ss-text-dim)]">{gapText(c)}</span>}
      {c.source && !c.automatic && <span className="text-[color:var(--ss-text-dim)]">· {c.source}</span>}
    </li>
  );
}

/** The run's expected values and hand calculations (VAL-35), as Results and
 *  Run info show them: the user's first, the automatic ones after. */
export function ReferenceList({ checks, compact = false }: { checks: ReferenceCheck[]; compact?: boolean }) {
  const mine = checks.filter((c) => !c.automatic);
  const auto = checks.filter((c) => c.automatic);
  if (checks.length === 0) return null;
  const failed = auto.filter((c) => c.grade !== "within");
  return (
    <div className="text-[11px]" role="region" aria-label="Expected values">
      {mine.length > 0 && (
        <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
          {mine.map((c, i) => (
            <CheckLine key={i} c={c} />
          ))}
        </ul>
      )}
      {auto.length > 0 &&
        (compact && failed.length === 0 ? (
          <details>
            <summary className="cursor-pointer text-[10px] text-[color:var(--ss-text-dim)]">
              Hand calculations: {auto.length} passed
            </summary>
            <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
              {auto.map((c, i) => (
                <CheckLine key={i} c={c} />
              ))}
            </ul>
          </details>
        ) : (
          <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
            {auto.map((c, i) => (
              <CheckLine key={i} c={c} />
            ))}
          </ul>
        ))}
    </div>
  );
}

/** Expected values of a case (VAL-35): numbers you trust (a maker's
 *  figure, last year's measured time, a hand calculation), each with a
 *  tolerance and a source; every run of the case shows how far it lands. */
export function ExpectedValuesEditor({ c }: { c: SimCase }) {
  const setCaseField = useProjectStore((s) => s.setCaseField);
  const runs = useProjectStore((s) => s.runs);
  const running = useProjectStore((s) => s.running);
  const id = useId();
  const refs = c.references ?? [];
  // the summary values of this case's latest run name what can be checked
  const last = [...runs].reverse().find((r) => r.caseId === c.id && r.result.summary.length > 0);
  const labels = last?.result.summary.map((s) => s.label) ?? [];
  const commit = (next: ReferenceValue[]) => setCaseField(c.id, { references: next });
  const patch = (i: number, p: Partial<ReferenceValue>) => commit(refs.map((r, j) => (j === i ? { ...r, ...p } : r)));
  const add = () => {
    const first = last?.result.summary.find((s) => !refs.some((r) => r.kpi === s.label));
    commit([...refs, { kpi: first?.label ?? "", value: first?.value ?? 0, tolerance: 5, tolerancePct: true, source: "" }]);
  };
  return (
    <div className="mb-3">
      <div className="mb-1 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
        Expected values · {c.name}
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[10px] font-normal normal-case tracking-normal"
          onClick={add}
          disabled={running}
          title="Add a number you trust for one of this case's results; every run then shows the gap"
        >
          <Plus size={10} /> Add
        </button>
      </div>
      {refs.length === 0 ? (
        <p className="mb-1 text-[11px] text-[color:var(--ss-text-dim)]">
          A number you trust for a result of this case, such as a maker's 0-100 km/h time or
          last year's measured 75 m time: each run shows how far it lands, green within the
          tolerance, amber within twice it, red beyond.
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          <datalist id={`${id}-kpis`}>
            {labels.map((l) => (
              <option key={l} value={l} />
            ))}
          </datalist>
          {refs.map((r, i) => (
            <li key={i} className="rounded border border-[color:var(--ss-border)] px-1.5 py-1 text-[11px]">
              <div className="flex min-w-0 items-center gap-1">
                <input
                  className="ss-input min-w-0 flex-1"
                  list={`${id}-kpis`}
                  aria-label="Result (summary value)"
                  title="The summary value to check, by its name in the results; the list holds this case's last run's values"
                  value={r.kpi}
                  onChange={(e) => patch(i, { kpi: e.target.value })}
                />
                <button
                  className="text-[color:var(--ss-text-dim)] hover:text-[color:var(--ss-err)]"
                  title="Remove this expected value"
                  onClick={() => commit(refs.filter((_, j) => j !== i))}
                >
                  <Trash2 size={12} />
                </button>
              </div>
              <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-1">
                <label className="flex items-center gap-1">
                  Expected
                  <input
                    type="number"
                    className="ss-input w-[72px]"
                    value={Number.isFinite(r.value) ? r.value : ""}
                    onChange={(e) => e.target.value !== "" && patch(i, { value: Number(e.target.value) })}
                  />
                </label>
                <label className="flex items-center gap-1">
                  ±
                  <input
                    type="number"
                    min={0}
                    className="ss-input w-[52px]"
                    aria-label="Tolerance"
                    value={Number.isFinite(r.tolerance) ? r.tolerance : ""}
                    onChange={(e) => e.target.value !== "" && patch(i, { tolerance: Math.abs(Number(e.target.value)) })}
                  />
                </label>
                <select
                  className="ss-input"
                  aria-label="Tolerance in"
                  value={r.tolerancePct === false ? "unit" : "pct"}
                  onChange={(e) => patch(i, { tolerancePct: e.target.value === "pct" })}
                >
                  <option value="pct">%</option>
                  <option value="unit">in its unit</option>
                </select>
              </div>
              <textarea
                className="ss-input mt-0.5 w-full resize-y"
                rows={r.source && r.source.length > 40 ? 3 : 1}
                aria-label="Source"
                placeholder="Source, e.g. maker's figure or FSG 2025 best time"
                value={r.source ?? ""}
                onChange={(e) => patch(i, { source: e.target.value })}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
