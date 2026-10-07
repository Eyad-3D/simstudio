// What held the car back at every moment (RES-38): a coloured band along
// the bottom of the Results chart, one strip per driveline, and a legend
// with the time spent in each state.
import type uPlot from "uplot";
import { LIMIT_LOOK, limitSegments, limitShares, lookColor } from "../../reports";
import { useProjectStore } from "../../store/projectStore";
import { useReportsStore } from "../../store/reportsStore";
import { useUIStore } from "../../store/uiStore";
import type { LimitReport, SimRun } from "../../types";
import type { PngPlugin } from "./Plot";

const STRIP = 7; // CSS px per driveline

function activeLimits(): LimitReport | null {
  const { runs, activeRunId } = useProjectStore.getState();
  if (!useReportsStore.getState().showLimits) return null;
  return runs.find((r) => r.id === activeRunId)?.result.limits ?? null;
}

/** Draws the band on a time axis (the band is in seconds, so not on a
 *  distance axis); `png` draws it on the saved picture too. */
export function limitsPlugin(kind: "t" | "distance"): PngPlugin {
  const draw = (k: number) => (u: uPlot) => {
    const lim = kind === "t" ? activeLimits() : null;
    if (!lim) return;
    const dark = useUIStore.getState().theme === "dark";
    const { ctx, bbox } = u;
    const px = devicePixelRatio * k;
    const h = STRIP * px;
    ctx.save();
    ctx.beginPath();
    ctx.rect(bbox.left, bbox.top, bbox.width, bbox.height);
    ctx.clip();
    lim.lanes.forEach((lane, li) => {
      const y = bbox.top + bbox.height - (lim.lanes.length - li) * (h + px);
      for (const [t0, t1, state] of limitSegments(lane, lim.states, lim.tEnd)) {
        const x0 = u.valToPos(t0, "x", true);
        const x1 = u.valToPos(t1, "x", true);
        if (x1 < bbox.left || x0 > bbox.left + bbox.width) continue;
        const look = LIMIT_LOOK[state];
        ctx.fillStyle = lookColor(look, dark);
        ctx.fillRect(x0, y, Math.max(x1 - x0, px), h);
        if (look?.hatch) {
          // the battery's own limit: the set limit's hue, hatched
          ctx.save();
          ctx.beginPath();
          ctx.rect(x0, y, x1 - x0, h);
          ctx.clip();
          ctx.strokeStyle = dark ? "#1b1f26" : "#ffffff";
          ctx.lineWidth = px;
          ctx.beginPath();
          for (let x = x0 - h; x < x1; x += 4 * px) {
            ctx.moveTo(x, y + h);
            ctx.lineTo(x + h, y);
          }
          ctx.stroke();
          ctx.restore();
        }
      }
    });
    ctx.restore();
  };
  return {
    hooks: { draw: draw(1) },
    png: (k) => ({ hooks: { draw: draw(k) } }),
  };
}

/** The band's legend: each state's colour, name and time, per driveline,
 *  and the switch that shows or hides the band. */
export function LimitLegend({ run, onTime }: { run: SimRun; onTime: boolean }) {
  const show = useReportsStore((s) => s.showLimits);
  const setShow = useReportsStore((s) => s.setShowLimits);
  const dark = useUIStore((s) => s.theme === "dark");
  const lim = run.result.limits;
  if (!lim || lim.lanes.length === 0) return null;
  return (
    <div
      role="region"
      aria-label="What limits the car"
      className="flex shrink-0 flex-col gap-0.5 border-t border-[color:var(--ss-border)] px-2 py-1 text-[11px]"
    >
      <label
        className="flex cursor-pointer items-center gap-1.5 text-[color:var(--ss-text-dim)]"
        title="A band along the bottom of the chart says, at every moment, what held the car back: the tyres' grip, the motor or engine, the battery or a set power limit. Each solver step counts as the first of these that applies (braking, grip, set limit, supply, motor or engine), else coasting or the driver's demand met."
      >
        <input type="checkbox" checked={show} onChange={(e) => setShow(e.target.checked)} />
        What limits the car{!onTime && show ? " (drawn on a time axis)" : ""}
      </label>
      {lim.lanes.map((lane) => (
        <div key={lane.label} className="flex flex-wrap items-center gap-x-3 gap-y-0.5">
          {lim.lanes.length > 1 && <span className="font-semibold">{lane.label}:</span>}
          {limitShares(lane).map(([state, s]) => {
            const look = LIMIT_LOOK[state];
            const color = lookColor(look, dark);
            return (
              <span key={state} className="flex items-center gap-1" title={look?.hint}>
                <span
                  className="inline-block h-2.5 w-3.5 rounded-sm"
                  aria-hidden="true"
                  style={{
                    background: look?.hatch
                      ? `repeating-linear-gradient(135deg, ${color} 0 3px, var(--ss-panel) 3px 4.5px)`
                      : color,
                  }}
                />
                {look?.label ?? state}
                <span className="font-mono text-[color:var(--ss-text-dim)]">
                  {s.toLocaleString(undefined, { maximumFractionDigits: s < 10 ? 2 : 1 })} s
                </span>
              </span>
            );
          })}
        </div>
      ))}
    </div>
  );
}
