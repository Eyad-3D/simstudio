import type { ParameterDef } from "./types";

/** A parameter's label without its note: "Charge Capacity (0 = …)" is
 *  "Charge Capacity", as Data Checks name it. */
export const paramName = (def: ParameterDef) => def.label.split(" (")[0];

/** A number parameter's limits in words ("above 0 and at most 100 %"), or
 *  null when it has none. */
export function limitsText(def: ParameterDef): string | null {
  const { minimum: low, exclusiveMinimum: above, maximum: high } = def;
  const parts = above != null ? [`above ${above}`] : low != null ? [`at least ${low}`] : [];
  if (high != null) parts.push(`at most ${high}`);
  return parts.length ? `${parts.join(" and ")}${def.unit === "-" ? "" : ` ${def.unit}`}` : null;
}

/** Why `value` breaks the parameter's limits ("must be above 0 and at most
 *  100 %"), or null. Same rule and words as ParameterDef.range_problem in
 *  backend/app/schemas.py, which Data Checks use; like them, text (NaN here)
 *  "is not a number". */
export function rangeProblem(def: ParameterDef, value: number): string | null {
  if (!Number.isFinite(value)) return "is not a number";
  const { minimum: low, exclusiveMinimum: above, maximum: high } = def;
  if ((low == null || value >= low) && (above == null || value > above) && (high == null || value <= high))
    return null;
  return `must be ${limitsText(def)}`;
}
