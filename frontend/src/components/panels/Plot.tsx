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
/** A plugin that also draws on the PNG picture, through `png(k)`: a copy of
 *  itself at k times the size, with no listeners. */
export type PngPlugin = uPlot.Plugin & { png?: (k: number) => uPlot.Plugin };

/** The page's font at `px` size, for text drawn on a canvas. */
export const font = (px: number) => `${px}px ${getComputedStyle(document.documentElement).fontFamily}`;

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

/** An axis range in a chart's accessible name: grouped, at most 3 decimals
 *  (read-outs use chartUtils' fmtNum). */
const fmt = (v: number | null | undefined) =>
  // (v === 0 ? 0 : v): -0 reads "0", not "-0"
  v == null ? "—" : (v === 0 ? 0 : v).toLocaleString(undefined, { maximumFractionDigits: 3 });

/** 1: time series, 2: X-Y (set at runtime, missing from the 1.6.32 typings) */
const modeOf = (u: uPlot) => (u as unknown as { mode: 1 | 2 }).mode;

/** The UI-scale setting is CSS zoom on an ancestor: pointer positions come in
 *  screen px while uPlot draws in CSS px, so they are divided by this. */
export const scaleOf = (el: HTMLElement) => el.getBoundingClientRect().width / el.offsetWidth || 1;

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
  } else if (f > 1) {
    // nor an X-Y axis past its data (or the view, where that is wider)
    lo = Math.max(Math.min(full[0], sc.min), lo);
    hi = Math.min(Math.max(full[1], sc.max), hi);
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
 *  labelled with what the chart shows, each y axis's range and, last, its x
 *  range ("Results chart: SOC; % 88.7 to 90.1; t [s] 0 to 600"). `onZoom`
 *  gets the time range while zoomed in, null for the whole run. */
function gestures(label: string, onZoom: (r: Range | null) => void): uPlot.Plugin {
  // the scales a gesture moves: x first, then on the X-Y view every y unit
  const scalesOf = (u: uPlot) =>
    modeOf(u) === 2 ? [...new Set(u.series.slice(1).flatMap((s) => s.facets!.map((f) => f.scale)))] : ["x"];
  const describe = (u: uPlot) => {
    // in the axis's unit: an axis drawn in min or km has a `div` (chartUtils' unitAxis)
    const range = (a: uPlot.Axis, key: string) => {
      const k = (a as { div?: number }).div ?? 1;
      const { min, max } = u.scales[key] ?? {};
      return `${fmt(min == null ? null : min / k)} to ${fmt(max == null ? null : max / k)}`;
    };
    const names = u.series.slice(1).map((s) => s.label);
    const ys = u.axes.slice(1).map((a) => `${a.label} ${range(a, a.scale ?? "y")}`);
    const xName = u.axes[0].label ?? u.series[0].label;
    u.root.setAttribute("aria-label", `${label}: ${names.join(", ")}; ${ys.join(", ")}; ${xName} ${range(u.axes[0], "x")}`);
  };
  // Shift+drag moves the view with the pointer
  const pan = (u: uPlot, e: MouseEvent) => {
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
      // a time chart with one sample (a live run's first flush) spans a
      // second from it, not the 0 to 100 uPlot pads it to
      scales:
        o.mode === 2
          ? o.scales
          : {
              ...o.scales,
              x: {
                ...o.scales?.x,
                range: (u, lo, hi) => (u.data[0].length === 1 ? [u.data[0][0], u.data[0][0] + 1] : [lo, hi]),
              },
            },
      cursor: {
        ...o.cursor,
        // a click that wobbles a pixel or two is not a zoom box
        drag: { dist: 5, ...o.cursor?.drag },
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
            // by how far the wheel turned (in px; a line is about 33, a page
            // 800), a notch of 100 px at most: a touchpad sends many small steps
            const dy = e.deltaY * (e.deltaMode === 1 ? 33 : e.deltaMode === 2 ? 800 : 1);
            const f = 1.25 ** Math.max(-1, Math.min(1, dy / 100));
            const [kx, ...ky] = scalesOf(u);
            u.batch(() => {
              zoomAround(u, kx, u.posToVal((e.clientX - r.left) / z, kx), f);
              ky.forEach((k) => zoomAround(u, k, u.posToVal((e.clientY - r.top) / z, k), f));
            });
          },
          { passive: false },
        );
        u.root.addEventListener("keydown", (e) => {
          if (e.ctrlKey || e.metaKey || e.altKey) return; // browser zoom and menus
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
        describe(u);
        if (key !== "x" || modeOf(u) === 2) return; // only a time zoom is kept, not an X-Y one
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
    // a plugin that draws on the chart (RES-06's cursors) gives a copy of
    // itself for the picture, drawn k times as large
    plugins: (o.plugins ?? []).flatMap((p: PngPlugin) => p.png?.(k) ?? []),
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
    // a series drawn as markers only (no line) gets a hollow marker
    const dots = s.paths?.(u, i + 1, 0, 0) === null;
    ctx.setLineDash(dots ? [] : (s.dash ?? []).map((d) => d * r));
    ctx.beginPath();
    if (dots) ctx.arc(place[i].x + 7 * r, y, 4 * r, 0, 2 * Math.PI);
    else {
      ctx.moveTo(place[i].x, y);
      ctx.lineTo(place[i].x + 14 * r, y);
    }
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

/** `png` saves the picture; `xRange` is the x range in view. */
export type PlotHandle = { png: (bg: string, name: string) => void; xRange: () => [number, number] | null };

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
      xRange: () => {
        const x = plot.current?.scales.x;
        return x?.min != null && x.max != null ? [x.min, x.max] : null;
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
    // and again when the legend gains a row (values fill in under the pointer)
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    const legendEl = u.root.querySelector(".u-legend");
    if (legendEl) ro.observe(legendEl);
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
