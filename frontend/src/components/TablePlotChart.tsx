// The chart of a table plot (TablePlot.tsx), on the charts' own uPlot
// wrapper. Loaded only when a plot is drawn.
import { useMemo, useState } from "react";
import uPlot from "uplot";
import { useUIStore } from "../store/uiStore";
import { PALETTE, fmtNum, yRange } from "./panels/chartUtils";
import { Plot, axisStyle, type PlotOptions } from "./panels/Plot";
import type { TableSlice } from "./TablePlot";

const colour = (i: number) => PALETTE[i % PALETTE.length];

export default function TablePlotChart({ slice }: { slice: TableSlice }) {
  const theme = useUIStore((s) => s.theme);
  // the pointer's place along x, and the curve nearest to it
  const [hover, setHover] = useState<{ idx: number | null; series: number | null }>({ idx: null, series: null });
  // a new chart only when what it is labelled with changes (a unit, the
  // axis along x, a curve more or less); an edit only sets new data
  const labels = JSON.stringify([slice.x.title, slice.yTitle, slice.curves.map((c) => c.label)]);
  const options = useMemo((): PlotOptions => {
    const [xTitle, yTitle, names] = JSON.parse(labels) as [string, string, string[]];
    const axis = axisStyle(theme);
    const ring = getComputedStyle(document.documentElement).getPropertyValue("--ss-text").trim();
    return {
      scales: { x: { time: false }, y: { range: yRange({}, uPlot.rangeNum) } },
      series: [
        { label: xTitle },
        ...names.map((label, i) => ({
          label,
          stroke: colour(i),
          width: 1.6,
          spanGaps: true,
          points: { show: true, size: 5, fill: colour(i) },
        })),
        // the selected cell's point: a ring, no line
        {
          label: "Selected cell",
          stroke: ring,
          paths: () => null,
          points: { show: true, size: 12, width: 2, stroke: ring, fill: "transparent" },
        },
      ],
      axes: [
        { ...axis, label: xTitle },
        { ...axis, label: yTitle, size: 52 },
      ],
      // the legend is drawn under the chart, as a list that wraps (a map
      // can have many curves); the line under it reads the pointer
      legend: { show: false },
      focus: { alpha: 0.35 },
      cursor: {
        // no zoom box: uPlot ends a drag on the main window's page, not on a
        // parameter window's (the wheel, the keys and a double-click still zoom)
        drag: { x: false, y: false },
        focus: { prox: 24 },
      },
      hooks: {
        setCursor: [(u) => setHover((h) => (h.idx === (u.cursor.idx ?? null) ? h : { ...h, idx: u.cursor.idx ?? null }))],
        setSeries: [(_u, i) => setHover((h) => ({ ...h, series: i ?? null }))],
      },
    };
  }, [labels, theme]);
  const shape = JSON.stringify(slice);
  const data = useMemo((): uPlot.AlignedData => {
    const s = JSON.parse(shape) as TableSlice;
    const marked = s.mark && s.curves[s.mark.curve]?.y[s.mark.index];
    const ring = s.x.values.map((_, i) => (s.mark && i === s.mark.index ? (marked ?? null) : null));
    return [s.x.values, ...s.curves.map((c) => c.y), ring] as uPlot.AlignedData;
  }, [shape]);

  // the read-out: the curve under the pointer (the selected cell's ring is its curve's)
  const n = slice.curves.length;
  const near = hover.series === null ? null : hover.series - 1 < n ? hover.series - 1 : (slice.mark?.curve ?? null);
  const k = n === 1 ? 0 : near;
  const i = hover.idx;
  const y = i === null || k === null ? null : slice.curves[k]?.y[i];
  const readout =
    i === null
      ? ""
      : `${slice.x.title} ${fmtNum(slice.x.values[i])}` +
        (k === null ? "" : ` · ${slice.curves[k].label}: ${y == null ? "no value" : `${fmtNum(y)} ${slice.yUnit}`.trim()}`);
  return (
    <>
      <div className="ss-table-plot-area">
        <Plot options={options} data={data} label="Table plot" />
      </div>
      {n > 1 && (
        <ul className="ss-table-plot-legend" aria-label="Curves">
          {slice.curves.map((c, j) => (
            <li key={j} className={near === j ? "ss-near" : undefined}>
              <span aria-hidden style={{ background: colour(j) }} />
              {c.label}
            </li>
          ))}
        </ul>
      )}
      <p className="ss-table-plot-readout">{readout || " "}</p>
    </>
  );
}
