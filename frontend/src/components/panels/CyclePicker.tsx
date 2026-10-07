import { useEffect, useState } from "react";
import * as api from "../../api";
import { useProjectStore } from "../../store/projectStore";

/** "WLTC class 3b · 1,800 s · 23.27 km" */
export const cycleText = (c: api.CycleInfo) =>
  `${c.name} · ${c.duration_s.toLocaleString("en")} s · ${c.distance_km.toFixed(2)} km`;

/** The Driving Task's Drive Cycle field (CON-16): the bundled standard cycles
 *  grouped by region, each with its length, or "" for the typed profile. The
 *  native list's type-ahead finds a cycle by its first letters. */
export function CycleSelect({
  value,
  label,
  description,
  onChange,
}: {
  value: string;
  label: string;
  /** the parameter's help text, read out with the field */
  description?: string | null;
  onChange: (cycleId: string) => void;
}) {
  const cycles = useProjectStore((s) => s.cycles);
  const regions = [...new Set(cycles.map((c) => c.region))];
  const chosen = cycles.find((c) => c.id === value);
  return (
    <select
      className="ss-input min-w-0 max-w-full flex-1"
      aria-label={label}
      aria-description={description ?? undefined}
      // a narrow panel can cut the chosen cycle's figures short
      title={chosen ? cycleText(chosen) : value || "Custom profile (typed points)"}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    >
      <option value="">Custom profile (typed points)</option>
      {regions.map((r) => (
        <optgroup key={r} label={r}>
          {cycles
            .filter((c) => c.region === r)
            .map((c) => (
              <option key={c.id} value={c.id}>
                {cycleText(c)}
              </option>
            ))}
        </optgroup>
      ))}
      {/* a newer file's cycle, or no list because the engine is not running */}
      {value && !cycles.some((c) => c.id === value) && (
        <option value={value}>
          {value}
          {cycles.length ? " (not in this version)" : ""}
        </option>
      )}
    </select>
  );
}

// traces fetched once per session: the bundled cycles never change while it runs
const traces = new Map<string, Promise<api.CycleTrace>>();

/** A small speed-over-time sketch of the chosen cycle, its phases marked, or
 *  of the typed profile, with its duration, distance and top speed. */
export function CyclePreview({
  cycleId,
  points,
  byDistance = false,
}: {
  cycleId: string;
  points: [number, number][];
  /** the typed profile is a speed against distance (m), not time (ENG-34) */
  byDistance?: boolean;
}) {
  const info = useProjectStore((s) => s.cycles.find((c) => c.id === cycleId));
  const [trace, setTrace] = useState<api.CycleTrace | null>(null);
  const id = info?.id;
  useEffect(() => {
    if (!id) return;
    let live = true;
    if (!traces.has(id)) traces.set(id, api.fetchCycle(id));
    traces.get(id)!.then(
      (t) => live && setTrace(t),
      () => {
        traces.delete(id); // try again next time
        if (live) setTrace(null);
      },
    );
    return () => {
      live = false;
    };
  }, [id]);
  // a cycle not in the list (a newer file's, or no engine) gets no sketch:
  // the typed profile is not what the task drives
  const pts: [number, number][] = cycleId
    ? info && trace?.id === info.id
      ? trace.t.map((t, i) => [t, trace.v[i]])
      : []
    : points;
  if (pts.length < 2) return null;
  const t0 = pts[0][0];
  const t1 = pts[pts.length - 1][0];
  const vmax = Math.max(...pts.map((p) => p[1]));
  const km = pts.slice(1).reduce((s, [t, v], i) => s + ((t - pts[i][0]) * (v + pts[i][1])) / 7200, 0);
  const W = 300;
  const H = 64;
  const x = (t: number) => ((t - t0) / (t1 - t0 || 1)) * W;
  const y = (v: number) => H - 2 - (v / (vmax || 1)) * (H - 4);
  // a typed profile over distance: its x axis is the distance itself
  const stats =
    byDistance && !cycleId
      ? `${(t1 - t0).toLocaleString("en")} m · top ${vmax.toFixed(1)} km/h`
      : `${(t1 - t0).toLocaleString("en")} s · ${km.toFixed(2)} km · top ${vmax.toFixed(1)} km/h`;
  return (
    <figure className="m-0">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        className="h-16 w-full rounded border border-[color:var(--ss-field-border)] bg-[color:var(--ss-panel-alt)]"
        role="img"
        aria-label={`Speed over ${byDistance && !cycleId ? "distance" : "time"}, ${info?.name ?? "custom profile"}: ${stats}`}
      >
        {info?.phases.slice(1).map(([name, start]) => (
          <line
            key={name}
            x1={x(start)}
            x2={x(start)}
            y1={0}
            y2={H}
            stroke="var(--ss-text-dim)"
            strokeDasharray="2 2"
            vectorEffect="non-scaling-stroke"
          >
            <title>{name}</title>
          </line>
        ))}
        <polyline
          points={pts.map(([t, v]) => `${x(t).toFixed(1)},${y(v).toFixed(1)}`).join(" ")}
          fill="none"
          stroke="var(--ss-accent)"
          strokeWidth={1.5}
          vectorEffect="non-scaling-stroke"
        />
      </svg>
      <figcaption className="mt-0.5 text-[11px] text-[color:var(--ss-text-dim)]">
        {info ? `${info.name}: ` : "Custom profile: "}
        {stats}
        {info?.phases.length ? ` · phases ${info.phases.map((p) => p[0]).join(", ")}` : ""}
      </figcaption>
    </figure>
  );
}
