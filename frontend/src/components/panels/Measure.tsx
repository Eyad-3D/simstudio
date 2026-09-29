// Measurement cursors A and B on the Results chart (RES-06): two lines on
// the chart, dragged with the mouse or set in two time fields, and a table of
// each plotted series' values at A and B and its statistics between them.
// A cursor is a time (s) on one of the primary run's samples, kept per run in
// uiStore, so overlaid runs are read at the same instants and the cursors
// stay while the view changes.
import { useMemo, useState } from "react";
import type uPlot from "uplot";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import type { Channel, SimRun } from "../../types";
import {
  INTEGRAL_UNITS,
  channelKey,
  distanceOf,
  firstReach,
  fmtNum,
  nearestIndex,
  timesOf,
  windowStats,
  type XAxis,
} from "./chartUtils";
import { font, scaleOf, type PngPlugin } from "./Plot";

type AB = [number, number];

/** A plotted series, as the Results panel lists them (one per run × channel). */
type Series = { dataKey: string; legend: string; unit: string; color: string; channel: Channel; run: SimRun };

/** A run's samples: n, the time of each, and where each sits on the chart's
 *  x axis (the time again, or the distance driven, which the sample's time
 *  finds through the index). */
function gridOf(run: SimRun, kind: XAxis["kind"]) {
  const ts = timesOf(run);
  const t = (i: number) => ts[i].t;
  const d = kind === "distance" ? (distanceOf(run) ?? []) : null;
  return { n: ts.length, t, x: d ? (i: number) => d[i + d.length - ts.length] : t };
}
type Grid = ReturnType<typeof gridOf>;

/** The sample time nearest x on the grid's axis. */
const timeAt = (g: Grid, x: number) => g.t(nearestIndex(g.n, x, g.x));
/** Where the sample at time t sits on the grid's axis. */
const xAt = (g: Grid, t: number) => g.x(nearestIndex(g.n, t, g.t));

/** A time to the ms, as the fields and Δt show it: 0.30000000000000004 is
 *  0.3, and a lap run's 30.1140480839 s is 30.114 (typed back, it finds the
 *  same sample). */
const ms = (t: number) => +t.toFixed(3) + 0;

/** Cursors at 1/4 and 3/4 of the x range in view (`range`, else the whole
 *  run), on the run's samples. */
export function cursorsIn(run: SimRun, kind: XAxis["kind"], range: [number, number] | null): AB {
  const g = gridOf(run, kind);
  const [lo, hi] = range ?? [g.x(0), g.x(g.n - 1)];
  return [timeAt(g, lo + (hi - lo) / 4), timeAt(g, lo + ((hi - lo) * 3) / 4)];
}

/** The primary run's cursors as a uPlot plugin on the Results chart: A and
 *  B as lines over the plot with the band between them shaded, drawn on
 *  every redraw (so they follow zoom, pan, resize and go into the PNG), and
 *  redrawn when they move. Dragging a line moves that cursor instead of
 *  drawing a zoom box. The run and its cursors are read when drawn: a live
 *  run's new samples must not make a new plugin, which rebuilds the chart. */
export function measurePlugin(kind: XAxis["kind"]): PngPlugin {
  const color = getComputedStyle(document.documentElement).getPropertyValue("--ss-accent").trim();
  let cache: { run: SimRun; g: Grid } | null = null;
  const state = () => {
    const { runs, activeRunId } = useProjectStore.getState();
    const run = runs.find((r) => r.id === activeRunId);
    const ab = run && useUIStore.getState().cursors[run.id];
    if (!run || !ab || timesOf(run).length === 0) return null;
    if (cache?.run !== run) cache = { run, g: gridOf(run, kind) };
    return { run, ab, g: cache.g };
  };
  const draw = (k: number) => (u: uPlot) => {
    const s = state();
    if (!s) return;
    const { ctx, bbox } = u;
    const w = devicePixelRatio * k;
    // a line of odd width sits on a pixel's middle, so it is drawn crisp
    const at = s.ab.map((t) => Math.round(u.valToPos(xAt(s.g, t), "x", true)) + (Math.round(w) % 2) / 2);
    ctx.save();
    ctx.setLineDash([]); // the last series drawn may have left a dash
    ctx.beginPath();
    ctx.rect(bbox.left, bbox.top, bbox.width, bbox.height);
    ctx.clip();
    ctx.fillStyle = color;
    ctx.globalAlpha = 0.08;
    ctx.fillRect(Math.min(...at), bbox.top, Math.abs(at[1] - at[0]), bbox.height);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.round(w);
    ctx.font = `bold ${font(11 * w)}`;
    ctx.textBaseline = "top";
    at.forEach((x, i) => {
      ctx.beginPath();
      ctx.moveTo(x, bbox.top);
      ctx.lineTo(x, bbox.top + bbox.height);
      ctx.stroke();
      ctx.fillText(i ? "B" : "A", x + 3 * w, bbox.top + 3 * w);
    });
    ctx.restore();
  };
  // the pointer's x in the plot's CSS px (the Results page may be zoomed)
  const plotX = (u: uPlot, e: MouseEvent) => (e.clientX - u.over.getBoundingClientRect().left) / scaleOf(u.over);
  // the cursor line within 6 px of x: 0 (A), 1 (B), or null
  const near = (u: uPlot, s: NonNullable<ReturnType<typeof state>>, x: number) => {
    const d = s.ab.map((t) => Math.abs(u.valToPos(xAt(s.g, t), "x") - x));
    const i = d[0] <= d[1] ? 0 : 1;
    return d[i] <= 6 ? i : null;
  };
  let unsubscribe = () => {};
  return {
    hooks: {
      init: (u) => {
        // on the way down, before uPlot's own handler on the plot, which
        // would start a zoom box
        u.root.addEventListener(
          "mousedown",
          (e) => {
            const s = e.button === 0 && e.target === u.over ? state() : null;
            const which = s && near(u, s, plotX(u, e));
            if (which == null) return;
            e.stopPropagation();
            e.preventDefault();
            const move = (m: MouseEvent) => {
              const now = state();
              if (!now) return;
              const next: AB = [...now.ab];
              next[which] = timeAt(now.g, u.posToVal(plotX(u, m), "x"));
              if (next[which] !== now.ab[which]) useUIStore.getState().setCursors(now.run.id, next);
            };
            const up = () => {
              window.removeEventListener("mousemove", move);
              window.removeEventListener("mouseup", up);
            };
            window.addEventListener("mousemove", move);
            window.addEventListener("mouseup", up);
          },
          true,
        );
        // nor a double-click on a line, which would show the whole run again
        u.root.addEventListener(
          "dblclick",
          (e) => {
            const s = e.target === u.over ? state() : null;
            if (s && near(u, s, plotX(u, e)) != null) e.stopPropagation();
          },
          true,
        );
        u.over.addEventListener("mousemove", (e) => {
          const s = state();
          u.over.style.cursor = s && near(u, s, plotX(u, e)) != null ? "ew-resize" : "";
        });
        unsubscribe = useUIStore.subscribe((s, p) => {
          if (s.cursors !== p.cursors) u.redraw(false);
        });
      },
      draw: draw(1),
      destroy: () => unsubscribe(),
    },
    png: (k) => ({ hooks: { draw: draw(k) } }),
  };
}

/** A series' values at A and B and its statistics between them, with A and
 *  B where x(i) (the sample's time, or its distance) reaches them; none at a
 *  cursor outside its samples (an overlaid run that ended sooner), and none
 *  at all when the window misses them. */
function measure(ts: Channel["timeSeries"], [pA, pB]: AB, x = (i: number) => ts[i].t) {
  const n = ts.length;
  if (n === 0 || Math.max(pA, pB) < x(0) || Math.min(pA, pB) > x(n - 1)) return null;
  const at = (p: number) => nearestIndex(n, p, x);
  const w = windowStats(ts, at(pA), at(pB));
  const inside = (p: number) => p >= x(0) && p <= x(n - 1);
  return w && { ...w, a: inside(pA) ? w.a : null, b: inside(pB) ? w.b : null };
}

/** A typed time in s, with a decimal point or comma; null when it is not one
 *  (Number() would take "0x10" as 16 and "" as 0). */
const parseTime = (text: string) =>
  /^\s*[+-]?(\d+[.,]?\d*|[.,]\d+)\s*$/.test(text) ? Number(text.replace(",", ".")) : null;

/** A cursor's time field: type a time (Enter or leaving the field sets it on
 *  the nearest sample), or step with the up and down arrows (a sample) and
 *  Page Up and Page Down (ten), from the typed time if there is one. */
function CursorInput({ run, ab, which }: { run: SimRun; ab: AB; which: 0 | 1 }) {
  // what was typed, and over which cursor time: once the cursor moves some
  // other way (a dragged line, time to reach), the typing is dropped
  const [draft, setDraft] = useState<{ text: string; over: number } | null>(null);
  const typed = draft?.over === ab[which] ? draft.text : null;
  const value = typed === null ? null : parseTime(typed);
  const ts = timesOf(run);
  const near = (t: number) => nearestIndex(ts.length, t, (k) => ts[k].t);
  const set = (t: number) => {
    const next: AB = [...ab];
    next[which] = t;
    useUIStore.getState().setCursors(run.id, next);
    setDraft(null);
  };
  // a time that is not one is dropped, as in the other number fields
  const commit = () => (value === null ? setDraft(null) : set(ts[near(value)].t));
  const name = which ? "B" : "A";
  const shown = String(ms(ab[which]));
  const invalid = typed !== null && value === null;
  return (
    <label className="flex items-center gap-1 font-semibold">
      {name}
      <input
        className="ss-input w-[80px] text-right font-mono font-normal"
        role="spinbutton"
        inputMode="decimal"
        aria-label={`Cursor ${name} time [s]`}
        aria-valuenow={ms(ab[which])}
        aria-valuemin={ms(ts[0].t)}
        aria-valuemax={ms(ts[ts.length - 1].t)}
        aria-invalid={invalid || undefined}
        title={invalid ? `Enter a time in s (leaving the field keeps ${shown})` : undefined}
        value={typed ?? shown}
        onFocus={(e) => e.currentTarget.select()}
        onChange={(e) => setDraft({ text: e.target.value, over: ab[which] })}
        onBlur={commit}
        onKeyDown={(e) => {
          const step = { ArrowUp: 1, ArrowDown: -1, PageUp: 10, PageDown: -10 }[e.key];
          if (step !== undefined) {
            e.preventDefault();
            const from = value === null ? near(ab[which]) : near(value);
            set(ts[Math.max(0, Math.min(ts.length - 1, from + step))].t);
          } else if (e.key === "Enter" && !invalid) commit();
          else if (e.key === "Escape") setDraft(null);
        }}
      />
      <span className="font-normal text-[color:var(--ss-text-dim)]">s</span>
    </label>
  );
}

/** Time to reach: puts A where a signal first reaches one value and B where
 *  it then first reaches another, so Δt is the time between them (0 to
 *  100 km/h) and the table shows what happened on the way. */
function TimeToReach({ run, channels }: { run: SimRun; channels: Channel[] }) {
  const [pick, setPick] = useState("");
  const [from, setFrom] = useState("0");
  const [to, setTo] = useState("100");
  const [note, setNote] = useState("");
  // a speed first: 0 to 100 km/h is the usual question
  const c = channels.find((x) => channelKey(x) === pick) ?? channels.find((x) => x.unit === "km/h") ?? channels[0];
  if (!c) return null;
  const name = c.label.split(" · ")[1] ?? c.label;
  const ok = from.trim() !== "" && to.trim() !== "" && Number.isFinite(Number(from)) && Number.isFinite(Number(to));
  const place = () => {
    const i0 = firstReach(c.timeSeries, Number(from));
    const i1 = i0 < 0 ? -1 : firstReach(c.timeSeries, Number(to), i0);
    if (i1 < 0) {
      setNote(i0 < 0 ? `${name} never reaches ${from} ${c.unit}.` : `${name} never reaches ${to} ${c.unit} after ${from} ${c.unit}.`);
      return;
    }
    setNote("");
    useUIStore.getState().setCursors(run.id, [c.timeSeries[i0].t, c.timeSeries[i1].t]);
  };
  const edit = (f: (v: string) => void) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    f(e.target.value);
    setNote("");
  };
  return (
    <form
      className="flex flex-wrap items-center gap-1"
      onSubmit={(e) => {
        e.preventDefault();
        place();
      }}
    >
      <label className="flex items-center gap-1">
        Time to reach
        <select className="ss-input max-w-[170px] py-0.5 text-[11px]" value={channelKey(c)} onChange={edit(setPick)}>
          {channels.map((x) => (
            <option key={channelKey(x)} value={channelKey(x)}>
              {x.label.split(" · ")[1] ?? x.label}
            </option>
          ))}
        </select>
      </label>
      <label className="flex items-center gap-1">
        from
        <input type="number" className="ss-input w-[64px]" value={from} onChange={edit(setFrom)} />
      </label>
      <label className="flex items-center gap-1">
        to
        <input type="number" className="ss-input w-[64px]" value={to} onChange={edit(setTo)} />
      </label>
      <span className="text-[color:var(--ss-text-dim)]">{c.unit}</span>
      <button type="submit" className="ss-toolbtn border border-[color:var(--ss-border)]" disabled={!ok}>
        Place A, B
      </button>
      <span role="status" className="text-[color:var(--ss-warn)]">
        {note}
      </span>
    </form>
  );
}

const HEADS: [string, string?][] = [
  ["A"],
  ["B"],
  ["B − A"],
  ["Min", "Lowest value between A and B"],
  ["Max", "Highest value between A and B"],
  ["Mean", "Mean between A and B, weighted by time"],
  ["RMS", "Root mean square between A and B, weighted by time: a typical size for a signal that swings up and down"],
  ["Integral", "Integral over time between A and B, for rates: kWh from kW, Ah from A, m from km/h, kg from kg/h, rev from 1/min"],
];

/** Under the chart while the primary run's cursors are on: the A and B time
 *  fields, Δt, time to reach, and a row per plotted series. */
export function MeasurePanel({ run, series, kind }: { run: SimRun; series: Series[]; kind: XAxis["kind"] }) {
  const ab = useUIStore((s) => s.cursors[run.id]);
  // on the distance axis, the lines cross an overlaid run where it had
  // driven as far as the primary run at A and B, not at the same times
  const dist = useMemo(() => {
    if (kind !== "distance") return null;
    const of = new Map<string, number[] | null>();
    for (const d of series) if (d.run.id !== run.id && !of.has(d.run.id)) of.set(d.run.id, distanceOf(d.run));
    return { g: gridOf(run, kind), of };
  }, [run, series, kind]);
  const rows = useMemo(() => {
    if (!ab) return [];
    return series.map((d) => {
      const ts = d.channel.timeSeries;
      const all = dist?.of.get(d.run.id);
      // (a channel that a live run began to send late ends on the same sample)
      const w =
        dist && all
          ? measure(ts, [xAt(dist.g, ab[0]), xAt(dist.g, ab[1])], (i) => all[i + all.length - ts.length])
          : measure(ts, ab);
      return { d, w };
    });
  }, [series, ab, dist]);
  if (!ab || timesOf(run).length === 0) return null;
  const cell = "ss-td whitespace-nowrap text-right font-mono";
  return (
    <section aria-label="Measurement cursors" className="shrink-0 border-t border-[color:var(--ss-border)] text-[11px]">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 bg-[color:var(--ss-panel-alt)] px-1.5 py-1">
        <CursorInput run={run} ab={ab} which={0} />
        <CursorInput run={run} ab={ab} which={1} />
        <span>
          Δt <span className="font-mono">{ms(ab[1] - ab[0]).toLocaleString()}</span> s
        </span>
        <TimeToReach run={run} channels={series.filter((d) => d.run.id === run.id).map((d) => d.channel)} />
      </div>
      <div role="region" aria-label="Cursor measurements" tabIndex={0} className="max-h-[150px] overflow-auto">
        <table className="w-full border-collapse">
          <thead className="sticky top-0">
            <tr>
              <th className="ss-th">Signal</th>
              <th className="ss-th">Unit</th>
              {HEADS.map(([h, title]) => (
                // (.ss-th's own alignment wins over a text-right class)
                <th key={h} className="ss-th" style={{ textAlign: "right" }} title={title}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map(({ d, w }) => {
              const int = INTEGRAL_UNITS[d.unit];
              return (
                <tr key={d.dataKey} className="hover:bg-[color:var(--ss-hover)]">
                  <td className="ss-td whitespace-nowrap">
                    <span className="mr-1 inline-block h-2 w-2 rounded-full" style={{ background: d.color }} aria-hidden="true" />
                    {d.legend}
                  </td>
                  <td className="ss-td text-[color:var(--ss-text-dim)]">{d.unit}</td>
                  <td className={cell}>{fmtNum(w?.a)}</td>
                  <td className={cell}>{fmtNum(w?.b)}</td>
                  <td className={cell}>{fmtNum(w?.a != null && w.b != null ? w.b - w.a : null)}</td>
                  <td className={cell}>{fmtNum(w?.min)}</td>
                  <td className={cell}>{fmtNum(w?.max)}</td>
                  <td className={cell}>{fmtNum(w?.mean)}</td>
                  <td className={cell}>{fmtNum(w?.rms)}</td>
                  <td className={cell}>{w && int ? `${fmtNum(w.integral * int[0])} ${int[1]}` : "—"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}
