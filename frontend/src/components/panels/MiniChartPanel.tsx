import { useMemo, useState } from "react";
import type uPlot from "uplot";
import { LineChart as LineChartIcon } from "lucide-react";
import { useActiveRun, useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { PALETTE, channelKey, fmtNum, fmtX, unitAxis, useHasSize, xAxisFor, yRange } from "./chartUtils";
import { Plot, axisStyle, type PlotOptions } from "./Plot";

/** A compact, dockable single-signal plot meant to sit beside the topology so a
 *  channel can be watched next to the diagram. It reads the active run, whose
 *  channels are assembled live during a run, so the trace updates in real time.
 *  For multi-channel / overlay / export use the full Results page. */
export function MiniChartPanel() {
  const activeRun = useActiveRun();
  const theme = useUIStore((s) => s.theme);
  const setRibbonTab = useUIStore((s) => s.setRibbonTab);
  const runsCount = useProjectStore((s) => s.runs.length);

  const channels = activeRun?.result.channels ?? [];
  const [picked, setPicked] = useState("");
  // the sample under the pointer (null: none), read out in the toolbar
  const [hover, setHover] = useState<number | null>(null);
  const { ref: chartRef, hasSize } = useHasSize<HTMLDivElement>();

  // the picked channel while the active run (whose channel set fills in live)
  // has it, else a sensible default
  const channel =
    channels.find((c) => channelKey(c) === picked) ??
    channels.find((c) => c.portId === "sig_soc") ??
    channels.find((c) => c.portId === "sig_power") ??
    channels[0] ??
    null;
  const sel = channel ? channelKey(channel) : "";
  const data = useMemo(
    (): uPlot.AlignedData =>
      channel ? [channel.timeSeries.map((p) => p.t), channel.timeSeries.map((p) => p.value)] : [[]],
    [channel],
  );
  const last = channel?.timeSeries.at(-1)?.value;
  const hovered = hover == null ? undefined : channel?.timeSeries[hover];
  const shortLabel = channel ? (channel.label.split(" · ")[1] ?? channel.label) : "";
  const unit = channel?.unit ?? "";
  // time in s, min or h by the run's length (RES-18)
  const x = xAxisFor("auto", activeRun ? [activeRun] : []);
  const options = useMemo((): PlotOptions => {
    const axis = axisStyle(theme);
    // this panel is often only a few lines tall: no legend or time-axis
    // title under the plot, the toolbar reads out the value under the pointer
    return {
      scales: { x: { time: false }, y: { range: yRange({}) } },
      series: [{ label: x.label }, { label: shortLabel, stroke: PALETTE[0], width: 1.6, spanGaps: true, points: { show: false } }],
      axes: [
        { ...axis, ...unitAxis(x.div), size: 24 },
        { ...axis, ...unitAxis(1), label: unit, size: 44 },
      ],
      legend: { show: false },
      cursor: { drag: { x: true, y: false } },
      hooks: { setCursor: [(u) => setHover(u.cursor.idx ?? null)] },
    };
  }, [shortLabel, unit, x.label, x.div, theme]);

  if (runsCount === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 px-4 text-center text-[color:var(--ss-text-dim)]">
        <LineChartIcon size={26} strokeWidth={1} />
        <div className="text-[12px]">Run a simulation to watch a signal here.</div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar">
        <LineChartIcon size={13} className="shrink-0 text-[color:var(--ss-text-dim)]" />
        <select
          className="ss-input min-w-0 flex-1 py-0.5 text-[11px]"
          value={sel}
          onChange={(e) => setPicked(e.target.value)}
          title="Channel to plot"
        >
          {channels.length === 0 && <option value="">No channels yet…</option>}
          {channels.map((c) => {
            const key = channelKey(c);
            return (
              <option key={key} value={key}>
                {c.label.split(" · ")[1] ?? c.label} [{c.unit}]
              </option>
            );
          })}
        </select>
        {hovered ? (
          <span className="shrink-0 whitespace-nowrap font-mono text-[11px] text-[color:var(--ss-text)]">
            t = {fmtX(x, hovered.t)} · {fmtNum(hovered.value)} {unit}
          </span>
        ) : (
          typeof last === "number" && (
            <span className="shrink-0 whitespace-nowrap font-mono text-[11px] text-[color:var(--ss-text)]">
              {fmtNum(last)} {unit}
            </span>
          )
        )}
        <button
          className="ss-toolbtn shrink-0 text-[11px]"
          title="Open the full Results page"
          onClick={() => setRibbonTab("results")}
        >
          Results…
        </button>
      </div>
      <div className="min-h-0 flex-1 p-1" ref={chartRef}>
        {channel && channel.timeSeries.length > 0 && hasSize ? (
          <Plot options={options} data={data} label="Signal Plot" />
        ) : (
          <div className="flex h-full items-center justify-center px-3 text-center text-[11px] text-[color:var(--ss-text-dim)]">
            {channels.length === 0
              ? "Waiting for run data…"
              : "Pick a channel to plot it here."}
          </div>
        )}
      </div>
    </div>
  );
}
