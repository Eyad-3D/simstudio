// The chart of a table plot (TablePlot.tsx), on the charts' own uPlot
// wrapper. Loaded only when a plot is drawn.
import { useMemo } from "react";
import uPlot from "uplot";
import { useUIStore } from "../store/uiStore";
import { PALETTE, fmtNum, yRange } from "./panels/chartUtils";
import { Plot, axisStyle, type PlotOptions } from "./panels/Plot";
import type { TableSlice } from "./TablePlot";

const value = (_u: uPlot, v: number | null) => fmtNum(v);

export default function TablePlotChart({ slice }: { slice: TableSlice }) {
  const theme = useUIStore((s) => s.theme);
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
        { label: xTitle, value },
        ...names.map((label, i) => {
          const colour = PALETTE[i % PALETTE.length];
          return { label, value, stroke: colour, width: 1.6, spanGaps: true, points: { show: true, size: 5, fill: colour } };
        }),
        // the selected cell's point: a ring, no line (its legend entry is hidden, index.css)
        {
          label: "Selected cell",
          value,
          stroke: ring,
          paths: () => null,
          points: { show: true, size: 12, width: 2, stroke: ring, fill: "transparent" },
        },
      ],
      axes: [
        { ...axis, label: xTitle },
        { ...axis, label: yTitle, size: 52 },
      ],
      // no zoom box: uPlot ends a drag on the main window's page, not on a
      // parameter window's (the wheel, the keys and a double-click still zoom)
      cursor: { drag: { x: false, y: false } },
    };
  }, [labels, theme]);
  const shape = JSON.stringify(slice);
  const data = useMemo((): uPlot.AlignedData => {
    const s = JSON.parse(shape) as TableSlice;
    const marked = s.mark && s.curves[s.mark.curve]?.y[s.mark.index];
    const ring = s.x.values.map((_, i) => (s.mark && i === s.mark.index ? (marked ?? null) : null));
    return [s.x.values, ...s.curves.map((c) => c.y), ring] as uPlot.AlignedData;
  }, [shape]);
  return <Plot options={options} data={data} label="Table plot" />;
}
