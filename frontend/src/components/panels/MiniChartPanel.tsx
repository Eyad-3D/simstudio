import { useMemo, useState } from "react";
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { LineChart as LineChartIcon } from "lucide-react";
import { useActiveRun, useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { PALETTE, channelKey, decimate, useHasSize } from "./chartUtils";

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
    () => (channel ? decimate(channel.timeSeries).map((pt) => ({ t: pt.t, v: pt.value })) : []),
    [channel],
  );
  const last = channel?.timeSeries.at(-1)?.value;
  const shortLabel = channel ? (channel.label.split(" · ")[1] ?? channel.label) : "";

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
        {typeof last === "number" && (
          <span className="shrink-0 whitespace-nowrap font-mono text-[11px] text-[color:var(--ss-text)]">
            {last.toLocaleString(undefined, { maximumFractionDigits: 3 })} {channel?.unit}
          </span>
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
        {data.length > 0 && hasSize ? (
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={data} margin={{ top: 6, right: 10, bottom: 2, left: 0 }}>
              <CartesianGrid stroke={theme === "dark" ? "#2a2f37" : "#eceff3"} />
              <XAxis
                dataKey="t"
                type="number"
                domain={["dataMin", "dataMax"]}
                tick={{ fontSize: 9 }}
                label={{ value: "t [s]", position: "insideBottomRight", fontSize: 9, offset: -2 }}
              />
              <YAxis
                tick={{ fontSize: 9 }}
                width={40}
                label={{
                  value: channel?.unit ?? "",
                  angle: -90,
                  position: "insideLeft",
                  fontSize: 9,
                  style: { textAnchor: "middle" },
                }}
              />
              <Tooltip
                cursor={{ stroke: "var(--ss-accent)", strokeWidth: 1, strokeDasharray: "3 3" }}
                contentStyle={{
                  fontSize: 11,
                  background: "var(--ss-panel)",
                  border: "1px solid var(--ss-border)",
                  color: "var(--ss-text)",
                }}
                labelFormatter={(t) => `t = ${t} s`}
                formatter={(value) => [
                  `${typeof value === "number" ? value.toLocaleString(undefined, { maximumFractionDigits: 3 }) : value} ${channel?.unit ?? ""}`,
                  shortLabel,
                ]}
              />
              <Line
                dataKey="v"
                type="linear"
                dot={false}
                strokeWidth={1.6}
                stroke={PALETTE[0]}
                isAnimationActive={false}
              />
            </LineChart>
          </ResponsiveContainer>
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
