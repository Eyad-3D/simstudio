import { useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { X } from "lucide-react";
import * as api from "../../api";
import { useProjectStore } from "../../store/projectStore";
import {
  addProjectCycle,
  cycleUsers,
  followProjectCycles,
  ownCycleInfo,
  ownCycleInfos,
  removeProjectCycle,
  renameProjectCycle,
} from "../../store/projectCycles";
import { cycleFigures, ImportCycleDialog } from "../ImportTableDialog";

// the store's cycle list carries the open project's own cycles (CON-11)
followProjectCycles();

/** "WLTC class 3b · 1,800 s · 23.27 km" (", with grade" when it has one);
 *  a cycle of one's own against distance: "Test lap · 800 m · top 70 km/h" */
export const cycleText = (c: api.CycleInfo) =>
  c.own
    ? `${c.name} · ${cycleFigures(c)}`
    : `${c.name} · ${c.duration_s.toLocaleString("en")} s · ${c.distance_km.toFixed(2)} km${c.grade ? " · with grade" : ""}`;

// the list's two actions: not cycles, so never a part's value
const IMPORT = "\u0000import";
const MANAGE = "\u0000manage";

/** Show a dialog in a React tree of its own, so its focus and key events
 *  do not reach the panel the list sits in (whose help card would take
 *  Esc); `close` removes it. */
function openDetached(render: (close: () => void) => ReactNode): void {
  const host = document.body.appendChild(document.createElement("div"));
  const root = createRoot(host);
  let open = true;
  const close = () => {
    if (!open) return;
    open = false;
    queueMicrotask(() => {
      root.unmount();
      host.remove();
    });
  };
  root.render(render(close));
}

/** The Driving Task's Drive Cycle field (CON-16): the bundled standard cycles
 *  grouped by region, each with its length, the project's own cycles
 *  (CON-11), or "" for the typed profile; and, at the end, importing a cycle
 *  from a file and managing the project's cycles. The native list's
 *  type-ahead finds a cycle by its first letters. */
export function CycleSelect({
  value,
  label,
  description,
  gradeOnly = false,
  onChange,
}: {
  value: string;
  label: string;
  /** the parameter's help text, read out with the field */
  description?: string | null;
  /** list only the cycles that carry a road grade (a Road Profile's list) */
  gradeOnly?: boolean;
  onChange: (cycleId: string) => void;
}) {
  const all = useProjectStore((s) => s.cycles);
  const ownCycles = useProjectStore((s) => s.project?.cycles);
  const editable = useProjectStore((s) => !s.readOnly && !s.offline && Boolean(s.project));
  const ownAll = useMemo(() => (ownCycles ?? []).map(ownCycleInfo), [ownCycles]);
  const bundled = all.filter((c) => !c.own);
  const cycles = gradeOnly ? bundled.filter((c) => c.grade) : bundled;
  const own = ownAll.filter((c) => (gradeOnly ? c.grade : c.speed));
  const regions = [...new Set(cycles.map((c) => c.region))];
  const chosen = [...cycles, ...own].find((c) => c.id === value);
  const known = ownAll.some((c) => c.id === value) || bundled.some((c) => c.id === value);
  const isOwn = value.startsWith("own:");
  return (
    <select
      className="ss-input min-w-0 max-w-full flex-1"
      aria-label={label}
      aria-description={description ?? undefined}
      // a narrow panel can cut the chosen cycle's figures short
      title={chosen ? cycleText(chosen) : value || "Custom profile (typed points)"}
      value={value}
      onChange={(e) => {
        const v = e.target.value;
        if (v === IMPORT)
          openDetached((close) => (
            <ImportCycleDialog
              onClose={close}
              onAdd={(cycle) => {
                const id = addProjectCycle(cycle);
                // pick it here when this list takes it
                if (id && (gradeOnly ? cycle.grade : cycle.speed)) onChange(id);
                return id;
              }}
            />
          ));
        else if (v === MANAGE) openDetached((close) => <ProjectCyclesDialog onClose={close} />);
        else onChange(v);
      }}
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
      {own.length > 0 && (
        <optgroup label="This project">
          {own.map((c) => (
            <option key={c.id} value={c.id}>
              {cycleText(c)}
            </option>
          ))}
        </optgroup>
      )}
      {/* a newer file's cycle, one the project no longer has, one this
          list does not take (a grade only), or no list (no engine) */}
      {value && !chosen && (
        <option value={value}>
          {ownAll.find((c) => c.id === value)?.name ?? value}
          {known ? "" : isOwn ? " (not in this project)" : all.length ? " (not in this version)" : ""}
        </option>
      )}
      {editable && (
        <optgroup label="Cycles of your own">
          <option value={IMPORT}>Import a cycle from a file…</option>
          {ownAll.length > 0 && <option value={MANAGE}>This project's cycles…</option>}
        </optgroup>
      )}
    </select>
  );
}

/** The project's own cycles: rename one, or remove one nothing uses. */
function ProjectCyclesDialog({ onClose }: { onClose: () => void }) {
  const project = useProjectStore((s) => s.project);
  const titleId = useId();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const list = ownCycleInfos(project);
  return (
    <div
      // over a parameter dialog, Esc closes this one only (ParameterDialog)
      className="ss-import-dialog fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="flex max-h-[86vh] w-[min(560px,calc(100vw-32px))] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="flex items-center gap-2 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-3 py-2">
          <span id={titleId} className="text-[13px] font-semibold">
            This project's drive cycles
          </span>
          <button className="ss-toolbtn ml-auto" title="Close (Esc)" aria-label="Close the dialog" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
        <ul className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3 text-[12px]">
          {list.length === 0 && (
            <li className="text-[color:var(--ss-text-dim)]">The project has no cycles of its own.</li>
          )}
          {list.map((c) => {
            const users = project ? cycleUsers(project, c.id) : [];
            return (
              <li key={c.id} className="flex flex-col gap-1 rounded border border-[color:var(--ss-border)] p-2">
                <div className="flex items-center gap-2">
                  <input
                    className="ss-input min-w-0 flex-1"
                    aria-label={`Name of ${c.name}`}
                    defaultValue={c.name}
                    maxLength={200}
                    onBlur={(e) => e.target.value.trim() !== c.name && renameProjectCycle(c.id, e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                  />
                  <button
                    className="ss-toolbtn border border-[color:var(--ss-border)] px-2 disabled:opacity-40"
                    disabled={users.length > 0}
                    title={
                      users.length
                        ? `Used by ${users.join(", ")}: pick another cycle there first`
                        : "Remove it from the project"
                    }
                    onClick={() => removeProjectCycle(c.id)}
                  >
                    Remove
                  </button>
                </div>
                <span className="text-[11px] text-[color:var(--ss-text-dim)]">
                  {c.axis === "distance" ? "Against distance" : "Against time"}: {cycleFigures(c)}
                  {c.source ? ` · from ${c.source}` : ""}
                  {users.length ? ` · used by ${users.join(", ")}` : " · not used"}
                </span>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}

/** Why LightSim may ship the cycle (its reuse basis, CON-31), in words. */
export const reuseText = (basis: string | undefined) =>
  ({
    "EU-2011/833": "EU legal text, reused under Decision 2011/833/EU",
    "US-17USC105": "US Government work",
    "JP-Art13": "Japanese official notice, free of copyright",
    "Apache-2.0": "Apache-2.0 copy",
    MIT: "MIT copy",
  })[basis ?? ""] ?? (basis || "reuse basis not recorded");

// traces fetched once per session: the bundled cycles never change while it runs
const traces = new Map<string, Promise<api.CycleTrace>>();

/** A small speed-over-time sketch of the chosen cycle, its phases marked, or
 *  of the typed profile, with its duration, distance and top speed. A cycle
 *  of the project's own is drawn from the project: against distance when it
 *  is, and its grade when it has no speed. */
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
  const info = useProjectStore((s) => s.cycles.find((c) => c.id === cycleId && !c.own));
  const ownCycle = useProjectStore((s) => s.project?.cycles?.find((c) => c.id === cycleId));
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
  const own = useMemo(() => (ownCycle ? ownCycleInfo(ownCycle) : null), [ownCycle]);
  const gradeOnly = Boolean(ownCycle && !ownCycle.speed);
  // a cycle not in the list (a newer file's, or no engine) gets no sketch:
  // the typed profile is not what the task drives
  const pts: [number, number][] = ownCycle
    ? ownCycle.x.map((x, i) => [x, (ownCycle.speed ?? ownCycle.grade ?? [])[i] ?? 0])
    : cycleId
      ? info && trace?.id === info.id
        ? trace.t.map((t, i) => [t, trace.v[i]])
        : []
      : points;
  if (pts.length < 2) return null;
  const t0 = pts[0][0];
  const t1 = pts[pts.length - 1][0];
  const vmax = Math.max(...pts.map((p) => p[1]));
  const vmin = Math.min(0, ...pts.map((p) => p[1]));
  const km = pts.slice(1).reduce((s, [t, v], i) => s + ((t - pts[i][0]) * (v + pts[i][1])) / 7200, 0);
  const W = 300;
  const H = 64;
  const x = (t: number) => ((t - t0) / (t1 - t0 || 1)) * W;
  const y = (v: number) => H - 2 - ((v - vmin) / (vmax - vmin || 1)) * (H - 4);
  const overDistance = own ? own.axis === "distance" : byDistance && !cycleId;
  // a typed profile over distance: its x axis is the distance itself
  const stats = own
    ? cycleFigures(own)
    : overDistance
      ? `${(t1 - t0).toLocaleString("en")} m · top ${vmax.toFixed(1)} km/h`
      : `${(t1 - t0).toLocaleString("en")} s · ${km.toFixed(2)} km · top ${vmax.toFixed(1)} km/h`;
  const name = own?.name ?? info?.name ?? "custom profile";
  return (
    <figure className="m-0">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        className="h-16 w-full rounded border border-[color:var(--ss-field-border)] bg-[color:var(--ss-panel-alt)]"
        role="img"
        aria-label={`${gradeOnly ? "Grade" : "Speed"} over ${overDistance ? "distance" : "time"}, ${name}: ${stats}`}
      >
        {info?.phases.slice(1).map(([name, start]) => (
          <line
            key={`${name}-${start}`}
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
        {own
          ? `${own.name} (this project's own, against ${own.axis === "distance" ? "distance" : "time"}): `
          : info
            ? `${info.name}: `
            : "Custom profile (not a standard cycle): "}
        {stats}
        {info?.phases.length ? ` · phases ${info.phases.map((p) => p[0]).join(", ")}` : ""}
        {own?.axis === "distance" && !gradeOnly && (
          <span className="block">Read against the distance the car has driven, whatever the Profile Axis says.</span>
        )}
        {info?.note && <span className="block">{info.note}</span>}
        {info?.source && (
          <span className="block" data-testid="cycle-source">
            Source: {info.source} ({reuseText(info.reuse)}; data register {info.register}).
          </span>
        )}
        {own && (
          <span className="block" data-testid="cycle-source">
            Source: {own.source || "your own data"} (kept in this project; not a standard cycle).
          </span>
        )}
      </figcaption>
    </figure>
  );
}
