import { useMemo } from "react";
import uPlot from "uplot";
import { fmtNum, fmtX, unitAxis, type XAxis } from "./chartUtils";
import { Plot, axisStyle, syncX, type PlotOptions } from "./Plot";

/** One line of the strip: a channel's difference, in the chart's unit. */
export type DiffLook = { legend: string; unit: string; color: string };

/** Zero, marked across the strip in each unit drawn. */
const zeroLine: uPlot.Plugin = {
  hooks: {
    draw: (u) => {
      const { ctx } = u;
      const { left, top, width, height } = u.bbox;
      ctx.save();
      ctx.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue("--ss-text-dim").trim();
      ctx.lineWidth = devicePixelRatio;
      ctx.setLineDash([4 * devicePixelRatio, 3 * devicePixelRatio]);
      for (const key of Object.keys(u.scales)) {
        if (key === "x" || !u.series.some((s, i) => i > 0 && s.scale === key)) continue;
        const y = u.valToPos(0, key, true);
        if (y < top || y > top + height) continue;
        ctx.beginPath();
        ctx.moveTo(left, Math.round(y) + 0.5);
        ctx.lineTo(left + width, Math.round(y) + 0.5);
        ctx.stroke();
      }
      ctx.restore();
    },
  },
};

/** A thin chart under the Results chart: the second run less the first for
 *  each channel drawn, on the chart's x values. Its y axes are the chart's
 *  (same units, sides and widths, so the two line up), each with zero in
 *  range; it zooms, pans and reads out with the chart. */
export function DiffStrip({
  data,
  looks,
  units,
  x,
  theme,
  syncKey,
  caption,
}: {
  data: uPlot.AlignedData;
  looks: DiffLook[];
  /** the chart's units, in the chart's order */
  units: string[];
  x: XAxis;
  theme: string;
  syncKey: string;
  caption: string;
}) {
  const looksText = JSON.stringify(looks);
  const unitsText = JSON.stringify(units);
  const xText = JSON.stringify(x);
  const options = useMemo((): PlotOptions => {
    const axis = axisStyle(theme);
    const ls: DiffLook[] = JSON.parse(looksText);
    const us: string[] = JSON.parse(unitsText);
    const xa: XAxis = JSON.parse(xText);
    // zero always in view: the strip is about the sign as much as the size
    const withZero = (_u: uPlot, lo: number | null, hi: number | null): uPlot.Range.MinMax => {
      if (lo == null || hi == null) return [-1, 1];
      const [a, b] = [Math.min(lo, 0), Math.max(hi, 0)];
      return a === b ? [-1, 1] : uPlot.rangeNum(a, b, 0.1, true);
    };
    return {
      scales: { x: { time: false }, ...Object.fromEntries(us.map((u) => [u, { range: withZero }])) },
      series: [
        {
          label: xa.kind === "t" ? "t" : "Distance",
          value: (_u, v) => (v == null ? "—" : fmtX(xa, v, null)),
        },
        ...ls.map((d) => ({
          label: d.legend,
          scale: d.unit,
          stroke: d.color,
          width: 1.4,
          spanGaps: true,
          points: { show: false },
          value: (_u: uPlot, v: number | null) => (v == null ? "—" : `${v > 0 ? "+" : ""}${fmtNum(v)} ${d.unit}`),
        })),
      ],
      axes: [
        { ...axis, ...unitAxis(xa.div), size: 20 },
        ...us.map((u, i) => ({
          ...axis,
          ...unitAxis(1),
          scale: u,
          label: u,
          side: i % 2 === 0 ? 3 : 1,
          size: 50,
          grid: { ...axis.grid, show: i === 0 },
        })),
      ],
      plugins: [zeroLine, syncX(syncKey)],
      cursor: { drag: { x: true, y: false }, sync: { key: syncKey, setSeries: false } },
    };
  }, [looksText, unitsText, xText, theme, syncKey]);
  return (
    <div className="flex h-[132px] shrink-0 flex-col px-1 pb-1">
      <div className="truncate px-1 text-meta text-[color:var(--ss-text-dim)]" title={caption}>
        {caption}
      </div>
      <div className="min-h-0 flex-1">
        <Plot key={`diff-${x.kind}`} options={options} data={data} label="Difference chart" />
      </div>
    </div>
  );
}
