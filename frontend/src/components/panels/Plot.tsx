// A thin React wrapper around uPlot (MIT, about 50 KB, draws on a canvas).
// Every chart in Results and the Signal Plot goes through it (RES-05).
//
// Gestures: drag draws a zoom box, the wheel zooms around the pointer,
// Shift+drag pans, double-click resets. On the focused chart: + and - zoom,
// the left and right arrows pan, 0 resets. Nothing is thinned away: uPlot
// draws the lowest and highest sample of each pixel column, so the full data
// goes in, a one-sample peak always shows and zooming in shows every sample.
import { useEffect, useImperativeHandle, useRef, type Ref } from "react";
import uPlot from "uplot";

type Range = { min: number; max: number };
/** uPlot's options without a size: a Plot fills its parent. */
export type PlotOptions = Omit<uPlot.Options, "width" | "height">;

/** The page's font at `px` size, for text drawn on a canvas. */
const font = (px: number) => `${px}px ${getComputedStyle(document.documentElement).fontFamily}`;

/** Axis colours from the theme tokens (a canvas cannot read CSS variables).
 *  `_theme` is there so that options memoised on the theme follow a switch. */
export function axisStyle(_theme: string): uPlot.Axis {
  const css = getComputedStyle(document.documentElement);
  const grid = css.getPropertyValue("--ss-td-border").trim();
  return {
    stroke: css.getPropertyValue("--ss-text-dim").trim(),
    font: font(10),
    labelFont: font(10),
    labelSize: 14,
    grid: { stroke: grid, width: 1 },
    ticks: { stroke: grid, width: 1, size: 4 },
  };
}

/** The charts' number format: grouped, at most `digits` decimals. */
export const fmt = (v: number | null | undefined, digits = 3) =>
  v == null ? "—" : v.toLocaleString(undefined, { maximumFractionDigits: digits });

/** 1: time series, 2: X-Y (set at runtime, missing from the 1.6.32 typings) */
const modeOf = (u: uPlot) => (u as unknown as { mode: 1 | 2 }).mode;

/** The UI-scale setting is CSS zoom on an ancestor: pointer positions come in
 *  screen px while uPlot draws in CSS px, so they are divided by this. */
const scaleOf = (el: HTMLElement) => el.getBoundingClientRect().width / el.offsetWidth || 1;

/** The lowest and highest value on a scale over all the data. */
function fullRange(u: uPlot, key: string): [number, number] | null {
  let lo = Infinity;
  let hi = -Infinity;
  const scan = (arr: ArrayLike<number | null | undefined>) => {
    for (let i = 0; i < arr.length; i++) {
      const v = arr[i];
      if (v == null) continue;
      if (v < lo) lo = v;
      if (v > hi) hi = v;
    }
  };
  if (modeOf(u) === 2) {
    u.series.forEach((s, i) => {
      if (i === 0) return;
      const d = u.data[i] as unknown as ArrayLike<number | null>[];
      if (s.facets![0].scale === key) scan(d[0]);
      if (s.facets![1].scale === key) scan(d[1]);
    });
  } else if (key === "x" && u.data[0].length) {
    lo = u.data[0][0];
    hi = u.data[0][u.data[0].length - 1];
  }
  return lo <= hi ? [lo, hi] : null;
}

function zoomAround(u: uPlot, key: string, at: number, f: number) {
  const sc = u.scales[key];
  const full = fullRange(u, key);
  if (sc.min == null || sc.max == null || !full) return;
  let lo = at - (at - sc.min) * f;
  let hi = at + (sc.max - at) * f;
  if (key === "x" && modeOf(u) === 1) {
    // time never zooms out past the run
    lo = Math.max(full[0], lo);
    hi = Math.min(full[1], hi);
  }
  if (hi > lo) u.setScale(key, { min: lo, max: hi });
}

function panBy(u: uPlot, key: string, delta: number, from: uPlot.Scale = u.scales[key]) {
  if (from.min == null || from.max == null) return;
  let lo = from.min + delta;
  const w = from.max - from.min;
  const full = fullRange(u, key);
  // time never pans past either end of the run
  if (full && key === "x" && modeOf(u) === 1) lo = Math.max(full[0], Math.min(lo, full[1] - w));
  u.setScale(key, { min: lo, max: lo + w });
}

/** The whole run again, y scales refitted. */
const reset = (u: uPlot) => u.setData(u.data, true);

/** Show a zoomed range again (after a rebuild or new data) if the data reaches into it. */
function restore(u: uPlot, range: Range | null) {
  const full = fullRange(u, "x");
  if (range && full && range.min < full[1] && range.max > full[0]) u.setScale("x", range);
}

/** The gestures, and a name for screen readers (and the tests): role img,
 *  labelled with what the chart shows and its x range. `onZoom` gets the time
 *  range while zoomed in, null for the whole run. */
function gestures(label: string, onZoom: (r: Range | null) => void): uPlot.Plugin {
  const scalesOf = (u: uPlot) =>
    modeOf(u) === 2 ? [u.series[1].facets![0].scale, u.series[1].facets![1].scale] : ["x"];
  const describe = (u: uPlot) => {
    const x = u.scales.x;
    const names = u.series.slice(1).map((s) => s.label);
    const xName = u.axes[0].label ?? u.series[0].label;
    u.root.setAttribute("aria-label", `${label}: ${names.join(", ")}; ${xName} ${fmt(x.min)} to ${fmt(x.max)}`);
  };
  // Shift+drag moves the view with the pointer
  const pan = (u: uPlot, e: MouseEvent) => {
    e.preventDefault();
    const z = scaleOf(u.over);
    const keys = scalesOf(u);
    const from = keys.map((k) => ({ ...u.scales[k] }));
    const move = (m: MouseEvent) =>
      u.batch(() =>
        keys.forEach((k, i) => {
          const px = i === 0 ? (e.clientX - m.clientX) / z : (m.clientY - e.clientY) / z;
          const size = i === 0 ? u.over.clientWidth : u.over.clientHeight;
          panBy(u, k, (px * (from[i].max! - from[i].min!)) / size, from[i]);
        }),
      );
    const up = () => {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", up);
    };
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", up);
  };
  return {
    opts: (_u, o) => ({
      ...o,
      cursor: {
        ...o.cursor,
        move: (u, left, top) => [left / scaleOf(u.over), top / scaleOf(u.over)],
        bind: {
          ...o.cursor?.bind,
          // a plain drag draws uPlot's zoom box; with Shift it pans
          mousedown: (u, targ, handler) => (e) => {
            if (e.button === 0 && e.target === targ) {
              if (e.shiftKey) pan(u, e);
              else handler(e);
            }
            return null;
          },
          // uPlot's own double-click resets x only; this refits y too
          dblclick: (u) => (e) => {
            if (e.button === 0) reset(u);
            return null;
          },
        },
      },
    }),
    hooks: {
      ready: (u) => {
        u.root.tabIndex = 0;
        u.root.setAttribute("role", "img");
        describe(u);
        u.over.addEventListener(
          "wheel",
          (e) => {
            if (e.deltaY === 0) return;
            e.preventDefault();
            const z = scaleOf(u.over);
            const r = u.over.getBoundingClientRect();
            const f = e.deltaY < 0 ? 0.8 : 1.25;
            const [kx, ky] = scalesOf(u);
            u.batch(() => {
              zoomAround(u, kx, u.posToVal((e.clientX - r.left) / z, kx), f);
              if (ky) zoomAround(u, ky, u.posToVal((e.clientY - r.top) / z, ky), f);
            });
          },
          { passive: false },
        );
        u.root.addEventListener("keydown", (e) => {
          const keys = scalesOf(u);
          const mid = (k: string) => ((u.scales[k].min ?? 0) + (u.scales[k].max ?? 0)) / 2;
          const span = (k: string) => (u.scales[k].max ?? 0) - (u.scales[k].min ?? 0);
          if (e.key === "+" || e.key === "=") u.batch(() => keys.forEach((k) => zoomAround(u, k, mid(k), 0.8)));
          else if (e.key === "-") u.batch(() => keys.forEach((k) => zoomAround(u, k, mid(k), 1.25)));
          else if (e.key === "ArrowLeft") panBy(u, keys[0], -span(keys[0]) / 10);
          else if (e.key === "ArrowRight") panBy(u, keys[0], span(keys[0]) / 10);
          else if (e.key === "0") reset(u);
          else return;
          e.preventDefault();
        });
      },
      setScale: (u, key) => {
        if (key !== "x") return;
        describe(u);
        if (modeOf(u) === 2) return; // an X-Y zoom is not kept
        const full = fullRange(u, "x");
        const { min, max } = u.scales.x;
        onZoom(full && min != null && max != null && (min > full[0] || max < full[1]) ? { min, max } : null);
      },
    },
  };
}

/** Double the picture: fonts, lines and spacing, for a PNG at 2x size. */
function scaled(o: PlotOptions, k: number): PlotOptions {
  const px = (f?: string) => f?.replace(/(\d+(\.\d+)?)px/, (_, n) => `${+n * k}px`);
  return {
    ...o,
    series: o.series.map((s) => ({
      ...s,
      width: (s.width ?? 1) * k,
      ...(s.points ? { points: { ...s.points, size: (s.points.size ?? 5) * k, width: (s.points.width ?? 1) * k } } : {}),
    })),
    axes: o.axes?.map((a) => ({
      ...a,
      font: px(a.font),
      labelFont: px(a.labelFont),
      size: typeof a.size === "number" ? a.size * k : a.size,
      labelSize: (a.labelSize ?? 30) * k,
      gap: (a.gap ?? 5) * k,
      // tick spacing too, or the copy draws twice as many ticks
      space: (typeof a.space === "number" ? a.space : a.scale && a.scale !== "x" ? 30 : 50) * k,
      grid: { ...a.grid, width: (a.grid?.width ?? 1) * k },
      ticks: { ...a.ticks, width: (a.ticks?.width ?? 1) * k, size: (a.ticks?.size ?? 10) * k },
    })),
    legend: { show: false },
    cursor: { show: false },
    hooks: {},
    plugins: [],
  };
}

/** The chart, its current zoom and a legend (swatches in the line colours,
 *  names in the text colour, wrapped to the width) as a PNG at twice the
 *  on-screen size, drawn from a throw-away copy of the chart. */
async function exportPng(u: uPlot, options: PlotOptions, bg: string, name: string) {
  const copy = new uPlot({ ...scaled(options, 2), width: u.width * 2, height: u.height * 2 }, u.data, document.createElement("div"));
  for (const key of Object.keys(u.scales)) {
    const s = u.scales[key];
    if (s.min != null && s.max != null) copy.setScale(key, { min: s.min, max: s.max });
  }
  await Promise.resolve(); // uPlot draws in a microtask
  const plot = copy.ctx.canvas;
  const r = plot.width / u.width; // 2, times the screen's pixel ratio
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  ctx.font = font(11 * r);
  // lay the legend out in rows first, to know the picture's height
  const items = options.series.slice(1).map((s) => ({ s, w: 22 * r + ctx.measureText(String(s.label ?? "")).width }));
  const rowH = 20 * r;
  const place: { x: number; row: number }[] = [];
  let x = 8 * r;
  let row = 0;
  for (const it of items) {
    if (x > 8 * r && x + it.w > plot.width) {
      x = 8 * r;
      row++;
    }
    place.push({ x, row });
    x += it.w + 12 * r;
  }
  canvas.width = plot.width;
  canvas.height = plot.height + (row + 1) * rowH;
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(plot, 0, 0);
  ctx.font = font(11 * r);
  ctx.textBaseline = "middle";
  ctx.lineWidth = 2 * r;
  const text = getComputedStyle(document.documentElement).getPropertyValue("--ss-text").trim();
  items.forEach(({ s }, i) => {
    const y = plot.height + place[i].row * rowH + rowH / 2;
    ctx.strokeStyle = typeof s.stroke === "string" ? s.stroke : text;
    ctx.setLineDash((s.dash ?? []).map((d) => d * r));
    ctx.beginPath();
    ctx.moveTo(place[i].x, y);
    ctx.lineTo(place[i].x + 14 * r, y);
    ctx.stroke();
    ctx.fillStyle = text;
    ctx.fillText(String(s.label ?? ""), place[i].x + 18 * r, y);
  });
  copy.destroy();
  canvas.toBlob((blob) => {
    if (!blob) return;
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${name}.png`;
    a.click();
    URL.revokeObjectURL(a.href);
  }, "image/png");
}

export type PlotHandle = { png: (bg: string, name: string) => void };

/** A chart that fills its parent, legend included. A new `options` builds a new chart, so memoise it on strings and numbers,
 *  never on a live run's objects, which change ten times a second. New `data`
 *  only redraws. A zoomed time range survives both, so ticking a channel or
 *  a live run's new samples keep the zoom. */
export function Plot({
  options,
  data,
  label,
  ref,
}: {
  options: PlotOptions;
  data: uPlot.AlignedData;
  label: string;
  ref?: Ref<PlotHandle>;
}) {
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const latest = useRef(data);
  const zoom = useRef<Range | null>(null);
  useImperativeHandle(
    ref,
    () => ({
      png: (bg, name) => {
        if (plot.current) void exportPng(plot.current, options, bg, name);
      },
    }),
    [options],
  );

  // in this order: the newest data, a new chart for new options (built with
  // that data), then new data into an existing chart
  useEffect(() => {
    latest.current = data;
  }, [data]);

  useEffect(() => {
    const el = host.current!;
    const kept = zoom.current;
    const u = new uPlot(
      {
        ...options,
        width: el.clientWidth,
        height: el.clientHeight,
        plugins: [...(options.plugins ?? []), gestures(label, (r) => (zoom.current = r))],
      },
      latest.current,
      el,
    );
    restore(u, kept);
    // the legend sits under the plot, inside the parent's height
    const fit = () => {
      const legend = u.root.querySelector<HTMLElement>(".u-legend")?.offsetHeight ?? 0;
      if (el.clientWidth > 0 && el.clientHeight > legend)
        u.setSize({ width: el.clientWidth, height: el.clientHeight - legend });
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    plot.current = u;
    return () => {
      ro.disconnect();
      u.destroy();
      plot.current = null;
    };
  }, [options, label]);

  useEffect(() => {
    const u = plot.current;
    if (!u || u.data === data) return; // a chart built with it already
    const kept = zoom.current;
    u.batch(() => {
      u.setData(data);
      restore(u, kept);
    });
  }, [data]);

  return <div ref={host} className="h-full w-full overflow-hidden" />;
}
