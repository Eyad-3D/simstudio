import { Fragment, useEffect, useEffectEvent, useId, useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import {
  ChartScatter,
  Download,
  Gauge,
  Image as ImageIcon,
  Info,
  Layers,
  LineChart as LineChartIcon,
  Play,
  Ruler,
  Search,
  SquareSplitHorizontal,
  Table2,
  TrendingUp,
  X,
  Zap,
} from "lucide-react";
import { downloadBlob, exportRunFile } from "../../api";
import { confirmDialog } from "../../dialog";
import { openHelp, useSummaryDefinition } from "../../help";
import { summaryChange } from "../../provenance";
import { previousRunOf, useActiveRun, useOverlayRuns, useProjectStore } from "../../store/projectStore";
import { useUIStore, type PlotView } from "../../store/uiStore";
import { sameFigure, type Channel, type SimRun, type SummaryValue } from "../../types";
import { useDismiss } from "../useDismiss";
import {
  PALETTE,
  channelKey,
  defaultChannelKeys,
  distanceOf,
  fmtNum,
  fmtX,
  headlineRows,
  mergeRows,
  pickSweepMetric,
  timesOf,
  unitAxis,
  useHasSize,
  xAxisFor,
  yRange,
  type XAxis,
  type XAxisMode,
  type YAxisCfg,
} from "./chartUtils";
import { csvBlob } from "./csv";
import { MeasurePanel, cursorsIn, measurePlugin } from "./Measure";
import { Plot, axisStyle, type PlotHandle, type PlotOptions } from "./Plot";
import { RunChanges, RunInfo, runLabel, runShort, runTime } from "./RunInfo";
import { DutyView } from "./DutyView";
import { EnergyView } from "./EnergyView";
import { LimitLegend, limitsPlugin } from "./LimitBand";
import { StaleBanner } from "./StaleBanner";
import { useReportsStore } from "../../store/reportsStore";
import { ReferenceList } from "./ExpectedValues";

// dash patterns to distinguish channels when several runs are overlaid at once
const DASHES = [[], [5, 3], [2, 2], [7, 3, 2, 3], [9, 4]];

/** Chart columns, with each row's sample time as `t` (on a distance axis the
 *  x column is not the time). */
type TimedData = uPlot.AlignedData & { t?: number[] };

const ESTIMATE = "An acceleration test's and a lap case's results are estimates: see the run's messages for why.";

// Table view rows have a fixed height, so only the rows in view are drawn
const TABLE_ROW_H = 26; // px
const TABLE_PAGE_ROWS = 80; // drawn before the first scroll event
const TABLE_OVERSCAN = 10;

/** A run's label for tooltips: a named run's clock time too. */
const runTitle = (r: SimRun) => (r.name ? `${runLabel(r)} · started ${runTime(r)}` : runLabel(r));

// a change against the baseline from this % up is set in bold (RES-10);
// neutral colour, as whether up is better depends on the value
const CHANGE_PCT = 1;
const NOISE = "Within the stored rounding: treat it as no change (see Known issues)";

/** A summary value against the baseline's (RES-10): the signed difference
 *  and % change as text, "~ 0" within the stored rounding, and `big` from
 *  CHANGE_PCT up; null when either run lacks the row. */
function changeOf(sv?: SummaryValue, base?: SummaryValue) {
  if (!sv || !base) return null;
  // (rows without a key come from a run stored before 0.3, which rounded them)
  const c = summaryChange(sv.value, base.value, !sv.key || !base.key);
  if (c.noise) return { diff: "~ 0", pct: "~ 0", noise: true, big: false };
  const signed = (v: number, digits: number) =>
    `${v > 0 ? "+" : ""}${v.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits })}`;
  return {
    // (no finer than the value columns, which show at most 3 decimals)
    diff: signed(c.diff, Math.min(c.digits, 3)),
    pct: c.pct === null ? "—" : `${signed(c.pct, Math.abs(c.pct) < 1 ? 2 : 1)} %`,
    noise: false,
    big: c.pct !== null && Math.abs(c.pct) >= CHANGE_PCT,
  };
}
const changeClass = (ch: ReturnType<typeof changeOf>) =>
  ch?.noise ? "text-[color:var(--ss-text-dim)]" : ch?.big ? "font-semibold text-[color:var(--ss-accent)]" : "";

// every unit's y axis fits its data until the user sets one
const AUTO_AXES: Record<string, YAxisCfg> = {};

/** A channel's times and values as chart columns, made once per sample array
 *  and length (a live run's arrays grow in place), so ticking a channel on a
 *  long run builds only that channel's columns, not every plotted series'. */
type Columns = { t: number[]; y: (number | null)[] };
const columnCache = new WeakMap<Channel["timeSeries"], Columns>();
function columnsOf(ts: Channel["timeSeries"]): Columns {
  const c = columnCache.get(ts);
  if (c && c.t.length === ts.length) return c;
  const fresh = { t: ts.map((p) => p.t), y: ts.map((p) => p.value) };
  columnCache.set(ts, fresh);
  return fresh;
}

/** A summary value's marks: its check's pass or fail and limit, and why it
 *  is not valid (spelled out with `why`; always in the tooltip). The table's
 *  value cells and the headline numbers share them. */
function SummaryMark({ sv, why }: { sv?: SummaryValue; why?: boolean }) {
  if (!sv) return null;
  return (
    <>
      {sv.passed != null && (
        <span
          className={`ml-1 rounded border px-1 font-sans text-[10px] font-semibold ${
            sv.passed
              ? "border-[color:var(--ss-ok)] text-[color:var(--ss-ok)]"
              : "border-[color:var(--ss-err)] text-[color:var(--ss-err)]"
          }`}
        >
          {sv.passed ? "pass" : "fail"}
        </span>
      )}
      {sv.limit != null && (
        <div className="font-sans text-[10px] text-[color:var(--ss-text-dim)]">≤ {sv.limit.toLocaleString()}</div>
      )}
      {sv.notValid && (
        <div className="truncate font-sans text-[10px] text-[color:var(--ss-warn)]" title={`Not valid: ${sv.notValid}`}>
          not valid{why && `: ${sv.notValid}`}
        </div>
      )}
    </>
  );
}

/** Per-unit y axis settings for the Chart and X-Y views (RES-18): each axis
 *  fits its data unless it starts at 0 or has an end set. */
function AxesMenu({
  units,
  value,
  onChange,
}: {
  units: string[];
  value: Record<string, YAxisCfg>;
  onChange: (v: Record<string, YAxisCfg>) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  useDismiss(open, () => setOpen(false), ref, button);
  const set = (u: string, cfg: YAxisCfg) => onChange({ ...value, [u]: cfg });
  const num = (s: string) => (s.trim() === "" || !Number.isFinite(Number(s)) ? undefined : Number(s));
  const id = useId();
  // units shown whose axis is not automatic: they stay so across runs and cases
  const fixed = units.filter((u) => value[u]?.zero || value[u]?.min != null || value[u]?.max != null).length;
  return (
    <div className="relative" ref={ref}>
      <button
        ref={button}
        className="ss-toolbtn border border-[color:var(--ss-border)]"
        aria-expanded={open}
        disabled={units.length === 0}
        title="Y axes: start one at 0, or set its ends"
        onClick={() => setOpen(!open)}
      >
        <Ruler size={12} /> Axes{fixed > 0 && ` (${fixed} set)`}
      </button>
      {open && (
        <div
          role="group"
          aria-label="Y axes"
          className="absolute right-0 top-full z-50 mt-1 flex w-[250px] flex-col gap-1.5 rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] p-2 text-[11px] shadow-lg"
        >
          {units.map((u, i) => {
            const cfg = value[u] ?? {};
            const crossed = cfg.min != null && cfg.max != null && cfg.min >= cfg.max;
            return (
              <fieldset key={u} className="rounded border border-[color:var(--ss-border)] px-1.5 pb-1.5">
                <legend className="px-1 font-semibold">{u}</legend>
                <div className="flex items-center gap-1.5">
                  <label className="flex items-center gap-1">
                    <input
                      type="checkbox"
                      checked={Boolean(cfg.zero)}
                      onChange={(e) => set(u, { ...cfg, zero: e.target.checked })}
                    />
                    Start at 0
                  </label>
                  <button
                    className="ml-auto rounded px-1 hover:bg-[color:var(--ss-hover)]"
                    title="Fit this axis to its data again"
                    onClick={() => set(u, {})}
                  >
                    Auto
                  </button>
                </div>
                <div className="mt-1 flex items-center gap-1">
                  {(["min", "max"] as const).map((end) => (
                    <input
                      key={end}
                      type="number"
                      className="ss-input w-0 min-w-0 flex-1"
                      aria-label={`${u} axis ${end === "min" ? "minimum" : "maximum"}`}
                      aria-invalid={crossed}
                      aria-describedby={crossed ? `${id}-${i}` : undefined}
                      placeholder={end === "min" ? "min: auto" : "max: auto"}
                      value={cfg[end] ?? ""}
                      onChange={(e) => set(u, { ...cfg, [end]: num(e.target.value) })}
                    />
                  ))}
                </div>
                {crossed && (
                  <div id={`${id}-${i}`} className="ss-param-problem mt-0.5">
                    The minimum must be below the maximum; until then both are automatic.
                  </div>
                )}
              </fieldset>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** The chosen channels of a run as CSV, the first column along the x axis:
 *  t_s as always, or t_min, t_h, or distance_m / distance_km with t_s next. */
function exportCsv(run: SimRun, keys: Set<string>, name: string, x: XAxis) {
  const channels = run.result.channels.filter((c) => keys.has(channelKey(c)));
  if (channels.length === 0) return;
  const dist = x.kind === "distance" ? distanceOf(run) : null;
  const header = [x.csv, ...(dist ? ["t_s"] : []), ...channels.map((c) => `${c.label} [${c.unit}]`)];
  // (to 12 digits: a time of 0.30000000000000004 s is 0.3)
  const clean = (v: number) => +v.toPrecision(12);
  const n = channels[0].timeSeries.length;
  const rows = channels[0].timeSeries.map((pt, i) => [
    ...(dist ? [clean(dist[i + dist.length - n] / x.div), clean(pt.t)] : [clean(pt.t / x.div)]),
    ...channels.map((c) => c.timeSeries[i]?.value ?? ""),
  ]);
  const blob = csvBlob([header, ...rows]);
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${name}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

/** The whole run for MATLAB or Python: every channel, its units and the run
 *  details, as a .mat file the engine writes (STD-09). */
async function exportMat(run: SimRun) {
  try {
    const { blob, name } = await exportRunFile(run, "mat");
    downloadBlob(blob, name);
  } catch (e) {
    useProjectStore.getState().log("error", `The .mat export failed: ${(e as Error).message}`);
  }
}

export function ResultsPanel() {
  const runs = useProjectStore((s) => s.runs);
  const activeRun = useActiveRun();
  const overlayRuns = useOverlayRuns();
  const overlayRunIds = useProjectStore((s) => s.overlayRunIds);
  const setActiveRun = useProjectStore((s) => s.setActiveRun);
  const toggleOverlayRun = useProjectStore((s) => s.toggleOverlayRun);
  const setOverlayRuns = useProjectStore((s) => s.setOverlayRuns);
  const clearOverlays = useProjectStore((s) => s.clearOverlays);
  const removeRun = useProjectStore((s) => s.removeRun);
  const storedRunCount = useProjectStore((s) => s.storedRunCount);
  const runsLoading = useProjectStore((s) => s.runsLoading);
  const running = useProjectStore((s) => s.running);
  const run = useProjectStore((s) => s.run);
  const caseName = useProjectStore((s) => s.project?.cases.find((c) => c.id === s.activeCaseId)?.name);
  const projectId = useProjectStore((s) => s.project?.id ?? "");
  const theme = useUIStore((s) => s.theme);
  const resultsView = useUIStore((s) => s.resultsViews[projectId]);

  const result = activeRun?.result;
  const selKey = activeRun?.caseId ?? "";

  // the choices the user made for this case: ticked channels, view, axes,
  // zoom, X-Y channel, sweep figure and baseline, kept across runs and
  // reloads (RES-19); unset ones take the defaults
  const plot = selKey ? resultsView?.cases?.[selKey] : undefined;
  const savePlot = (patch: Partial<PlotView>) => {
    if (projectId && selKey) useUIStore.getState().setPlotView(projectId, selKey, patch);
  };
  const savedChannels = plot?.channels;
  const sweepPick = plot?.sweepMetric ?? "";
  const xyXPick = plot?.xKey ?? ""; // channel picked as the X axis in the X-Y view
  // the chart's x axis (time in s, min or h, or distance) and each unit's y axis
  const xAxisMode = plot?.xAxis ?? "auto";
  const yAxes = plot?.yAxes ?? AUTO_AXES;
  const comparePrevious = resultsView?.comparePrevious ?? true;
  const [search, setSearch] = useState("");
  const [showRunInfo, setShowRunInfo] = useState(false);
  const { ref: chartHost, hasSize } = useHasSize<HTMLDivElement>();
  const plotRef = useRef<PlotHandle>(null);
  // the primary run's measurement cursors (RES-06); their positions live in
  // uiStore, so moving one redraws only the chart and the measurement table
  const cursorsOn = useUIStore((s) => Boolean(activeRun && s.cursors[activeRun.id]));

  // sensible default channel selection until the user picks for this case
  // (a live run starts with no channels, so it fills in as they arrive)
  const defaultKeys = useMemo(() => defaultChannelKeys(result?.channels ?? []), [result]);

  const selectedKeys = useMemo(
    () => new Set(selKey ? (savedChannels ?? defaultKeys) : []),
    [savedChannels, selKey, defaultKeys],
  );
  const selectedList = useMemo(() => [...selectedKeys], [selectedKeys]);

  // sweep family of the active run (sorted by swept value)
  const family = useMemo(() => {
    if (!activeRun?.sweepId) return [];
    return runs
      .filter((r) => r.sweepId === activeRun.sweepId)
      .sort((a, b) => (a.sweepValue ?? 0) - (b.sweepValue ?? 0));
  }, [runs, activeRun]);
  // stopped/failed points are only drawn (hollow) when the user asks for them
  const [showIncomplete, setShowIncomplete] = useState(false);
  const completeFamily = useMemo(() => family.filter((r) => !r.incomplete), [family]);
  const incompleteCount = family.length - completeFamily.length;
  // (a Sweep view kept for the case shows the chart for a run that is not a sweep point)
  const view = plot?.view === "sweep" && family.length < 2 ? "chart" : (plot?.view ?? "chart");
  const setView = (v: typeof view) => savePlot({ view: v });
  const report = view === "energy" || view === "duty"; // the run's reports (RES-22, RES-39)
  const canMeasure = view !== "sweep" && !report && Boolean(activeRun && timesOf(activeRun).length > 0);
  const showLimits = useReportsStore((s) => s.showLimits);

  const byElement = useMemo(() => {
    const q = search.trim().toLowerCase();
    const groups = new Map<string, Channel[]>();
    for (const c of result?.channels ?? []) {
      if (q && !c.label.toLowerCase().includes(q) && !c.unit.toLowerCase().includes(q)) continue;
      const el = c.label.split(" · ")[0];
      if (!groups.has(el)) groups.set(el, []);
      groups.get(el)!.push(c);
    }
    return [...groups.entries()];
  }, [result, search]);

  // runs plotted together: the active run first, then each overlay
  const plotRuns = useMemo(
    () => [activeRun, ...overlayRuns].filter((r): r is SimRun => Boolean(r)),
    [activeRun, overlayRuns],
  );
  const multiRun = plotRuns.length > 1;
  // the run the numbers are compared with (RES-10): the one picked for the
  // case while it is listed, else the previous run of the case, or none
  const previous = activeRun ? previousRunOf(activeRun.caseId, runs, activeRun.startedAt) : undefined;
  const pick = plot?.baselineRunId;
  const picked = pick ? runs.find((r) => r.id === pick && r.id !== activeRun?.id) : undefined;
  const baseline = pick === null ? undefined : (picked ?? previous);
  // drawn faint with the lines, unless it is overlaid already (RES-19)
  const ghost = comparePrevious && baseline && !overlayRunIds.includes(baseline.id) ? baseline : undefined;
  const drawnRuns = useMemo(() => (ghost ? [...plotRuns, ghost] : plotRuns), [plotRuns, ghost]);
  const headline = useMemo(() => headlineRows(result?.summary ?? []), [result]);
  const estimate = activeRun?.snapshot?.case.kind === "acceleration" || activeRun?.snapshot?.case.kind === "lap";
  const define = useSummaryDefinition(); // each row's hover text (LRN-10)
  const runColor = (i: number) => PALETTE[i % PALETTE.length];
  const channelColor = (key: string) => PALETTE[Math.max(0, selectedList.indexOf(key)) % PALETTE.length];
  const overlayColorOf = (id: string) => {
    const idx = overlayRunIds.indexOf(id);
    return idx >= 0 ? runColor(idx + 1) : "#c0c6d0";
  };

  // one plotted series per (run × selected channel), the baseline's last
  const seriesDefs = useMemo(() => {
    const defs: {
      dataKey: string;
      unit: string;
      color: string;
      dash?: number[];
      faint?: boolean;
      legend: string;
      channel: Channel;
      run: SimRun;
    }[] = [];
    drawnRuns.forEach((r, ri) => {
      const faint = r === ghost;
      for (const c of r.result.channels) {
        const key = channelKey(c);
        // (the target speed is not what a comparison is about)
        if (!selectedKeys.has(key) || (faint && c.portId === "sig_demand")) continue;
        const ci = selectedList.indexOf(key);
        const chLabel = c.label.split(" · ")[1] ?? c.label;
        defs.push({
          dataKey: `${r.id}::${key}`,
          unit: c.unit,
          // the baseline in the primary run's colour, dashed and faint
          color: multiRun ? runColor(faint ? 0 : ri) : channelColor(key),
          // one run: a speed target is dashed, so it still shows where the car follows it
          dash: faint
            ? [4, 3]
            : multiRun
              ? selectedList.length > 1
                ? DASHES[ci % DASHES.length]
                : undefined
              : c.portId === "sig_demand"
                ? [5, 3]
                : undefined,
          faint,
          legend: faint ? `baseline · ${chLabel}` : multiRun ? `${runShort(r)} · ${chLabel}` : chLabel,
          channel: c,
          run: r,
        });
      }
    });
    // and drawn last, over the speed that follows it
    const target = (d: (typeof defs)[number]) => Number(!multiRun && d.channel.portId === "sig_demand");
    return defs.sort((a, b) => target(a) - target(b));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drawnRuns, ghost, selectedKeys, selectedList, multiRun]);

  // the x axis: time in the chosen unit, or the distance driven, which needs
  // a Vehicle in every plotted run (else it is time, picked automatically)
  const canDistance =
    drawnRuns.length > 0 && drawnRuns.every((r) => r.result.channels.some((c) => c.portId === "sig_distance"));
  const xMode = xAxisMode === "distance" && !canDistance ? "auto" : xAxisMode;
  const xAxis = useMemo(() => xAxisFor(xMode, drawnRuns), [xMode, drawnRuns]);

  // uPlot columns: one x column (in s or m), one column per series. Series
  // on the same grid (a run's channels; on a time axis overlaid runs of the
  // same case too) share it as it is; other grids are merged by x and time,
  // leaving gaps the lines span. The samples' times go along as `t`, for the
  // read-out on a distance axis.
  const chartData = useMemo((): TimedData => {
    if (view !== "chart" || seriesDefs.length === 0) return [[]];
    const dist = new Map<string, number[] | null>();
    for (const d of seriesDefs)
      if (!dist.has(d.run.id)) dist.set(d.run.id, xAxis.kind === "distance" ? distanceOf(d.run) : null);
    const series = seriesDefs.map((d) => {
      const { t, y } = columnsOf(d.channel.timeSeries);
      const all = dist.get(d.run.id);
      // a channel that a live run began to send late ends on the same sample
      const x = all ? t.map((_, j) => all[j + all.length - t.length]) : t;
      const grid = `${all ? d.run.id : ""}:${t.length}:${t[0]}:${t[t.length - 1]}`;
      return { x, t, y, grid };
    });
    if (series.every((s) => s.grid === series[0].grid))
      return Object.assign([series[0].x, ...series.map((s) => s.y)] as uPlot.AlignedData, { t: series[0].t });
    const merged = mergeRows(series);
    return Object.assign([merged.x, ...merged.cols] as uPlot.AlignedData, { t: merged.t });
  }, [seriesDefs, view, xAxis.kind]);
  // what the series and axes look like, as strings: a live run replaces its
  // channel objects on every flush, and options built from them would rebuild
  // the chart (and drop a zoom box being drawn) ten times a second
  const seriesLook = JSON.stringify(
    seriesDefs.map(({ legend, unit, color, dash, faint }) => ({ legend, unit, color, dash, faint })),
  );
  const xLook = JSON.stringify(xAxis);
  const yLook = JSON.stringify(yAxes);
  const chartOptions = useMemo((): PlotOptions => {
    const axis = axisStyle(theme);
    const looks: { legend: string; unit: string; color: string; dash?: number[]; faint?: boolean }[] =
      JSON.parse(seriesLook);
    const x: XAxis = JSON.parse(xLook);
    const ys: Record<string, YAxisCfg> = JSON.parse(yLook);
    const units = [...new Set(looks.map((d) => d.unit))];
    return {
      scales: { x: { time: false }, ...Object.fromEntries(units.map((u) => [u, { range: yRange(ys[u] ?? {}, uPlot.rangeNum) }])) },
      series: [
        {
          label: x.kind === "t" ? "t" : "Distance",
          value: (u, v, _s, i) => (v == null ? "—" : fmtX(x, v, i == null ? null : (u.data as TimedData).t?.[i])),
        },
        ...looks.map((d) => ({
          label: d.legend,
          scale: d.unit,
          stroke: d.color,
          width: d.faint ? 1.2 : 1.6,
          ...(d.faint ? { alpha: 0.45 } : {}),
          dash: d.dash,
          spanGaps: true,
          points: { show: false },
          value: (_u: uPlot, v: number | null) => (v == null ? "—" : `${fmtNum(v)} ${d.unit}`),
        })),
      ],
      // one y axis per unit, on alternating sides
      axes: [
        { ...axis, ...unitAxis(x.div), label: x.label, size: 24 },
        ...units.map((u, i) => ({
          ...axis,
          ...unitAxis(1),
          scale: u,
          label: u,
          side: i % 2 === 0 ? 3 : 1,
          size: 50,
          grid: { ...axis.grid, show: i === 0 },
        })),
      ],
      plugins: [measurePlugin(x.kind), limitsPlugin(x.kind)],
      cursor: {
        drag: { x: true, y: false },
        // runs on other time grids have gaps in the merged columns: read the
        // nearest sample of the series, but none before its first or after
        // its last (a shorter run has ended there)
        dataIdx: (u, s, i) => {
          const ys = u.data[s];
          let lo = i;
          let hi = i;
          while (lo >= 0 && ys[lo] === undefined) lo--;
          while (hi < ys.length && ys[hi] === undefined) hi++;
          if (lo < 0 || hi >= ys.length) return null;
          const x = u.data[0][i];
          return x - u.data[0][lo] <= u.data[0][hi] - x ? lo : hi;
        },
      },
    };
    // (the limit band is drawn from the store: a switch redraws it)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seriesLook, xLook, yLook, theme, showLimits]);

  // table shows only the active run (aligned time grid), every sample; only
  // the rows scrolled into view are drawn (see onTableScroll)
  const activeChannels = useMemo(
    () => result?.channels.filter((c) => selectedKeys.has(channelKey(c))) ?? [],
    [result, selectedKeys],
  );
  const tableData = useMemo(() => {
    if (view !== "table" || activeChannels.length === 0) return [];
    return activeChannels[0].timeSeries.map((pt, i) => {
      const row: Record<string, number | null> = { t: pt.t };
      activeChannels.forEach((c) => {
        const p = c.timeSeries[i];
        if (p) row[channelKey(c)] = p.value;
      });
      return row;
    });
  }, [activeChannels, view]);
  const [tableWindow, setTableWindow] = useState({ first: 0, count: TABLE_PAGE_ROWS });
  const onTableScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    // work in fractions of the scroll height so the UI zoom cannot skew it
    const rows = ((tableData.length + 1) * el.scrollTop) / el.scrollHeight;
    const shown = ((tableData.length + 1) * el.clientHeight) / el.scrollHeight;
    const first = Math.max(0, Math.floor(rows) - TABLE_OVERSCAN);
    const count = Math.max(TABLE_PAGE_ROWS, Math.ceil(shown) + 2 * TABLE_OVERSCAN);
    if (first !== tableWindow.first || count !== tableWindow.count) setTableWindow({ first, count });
  };
  const tableFirst = Math.min(tableWindow.first, tableData.length);
  const tableLast = Math.min(tableData.length, tableFirst + tableWindow.count);

  // --- X-Y (channel-vs-channel) plot: active run only, samples aligned by index.
  // The left-hand checkboxes pick the channels; one of them is the X axis, the
  // rest are plotted as Y series against it (e.g. motor torque vs. motor speed).
  const channelByKey = useMemo(() => {
    const m = new Map<string, Channel>();
    for (const c of result?.channels ?? []) m.set(channelKey(c), c);
    return m;
  }, [result]);

  // the ticked channels this run has (a kept pick can name a part deleted since)
  const shownKeys = useMemo(() => selectedList.filter((k) => channelByKey.has(k)), [selectedList, channelByKey]);
  // the picked X channel while it is shown, else the first shown one that is
  // not a speed target (plotting against the target means little)
  const xyXKey = shownKeys.includes(xyXPick)
    ? xyXPick
    : (shownKeys.find((k) => channelByKey.get(k)?.portId !== "sig_demand") ?? shownKeys[0] ?? "");

  const xyXChannel = xyXKey ? (channelByKey.get(xyXKey) ?? null) : null;
  const xyYChannels = useMemo(
    () => activeChannels.filter((c) => channelKey(c) !== xyXKey),
    [activeChannels, xyXKey],
  );
  const xyXShort = xyXChannel ? (xyXChannel.label.split(" · ")[1] ?? xyXChannel.label) : "";
  const xyXLabel = `${xyXShort}${xyXChannel?.unit ? ` [${xyXChannel.unit}]` : ""}`;
  // X-Y: every sample, X and each Y paired by index (uPlot's mode 2)
  const xyData = useMemo((): uPlot.AlignedData => {
    if (view !== "xy" || !xyXChannel || xyYChannels.length === 0) return [null] as unknown as uPlot.AlignedData;
    const xs = xyXChannel.timeSeries.map((p) => p.value);
    return [null, ...xyYChannels.map((c) => [xs, c.timeSeries.map((p) => p.value)])] as unknown as uPlot.AlignedData;
  }, [view, xyXChannel, xyYChannels]);
  const xyLook = JSON.stringify(
    xyYChannels.map((c) => ({ legend: c.label.split(" · ")[1] ?? c.label, unit: c.unit, color: channelColor(channelKey(c)) })),
  );
  const xyXUnit = xyXChannel?.unit ?? "";
  const xyOptions = useMemo((): PlotOptions => {
    const axis = axisStyle(theme);
    const looks: { legend: string; unit: string; color: string }[] = JSON.parse(xyLook);
    const ys: Record<string, YAxisCfg> = JSON.parse(yLook);
    const units = [...new Set(looks.map((d) => d.unit))];
    const pair = (u: uPlot, s: number) => u.data[s] as unknown as (number | null)[][];
    return {
      mode: 2,
      scales: { x: { time: false }, ...Object.fromEntries(units.map((u) => [u, { range: yRange(ys[u] ?? {}, uPlot.rangeNum) }])) },
      series: [
        {},
        ...looks.map((d) => ({
          label: d.legend,
          stroke: d.color,
          width: 1.6,
          facets: [
            { scale: "x", auto: true },
            { scale: d.unit, auto: true },
          ],
          // the hovered point, as "x → y"
          value: (u: uPlot, _v: number | null, s: number, i: number | null) =>
            i == null ? "—" : `${fmtNum(pair(u, s)[0][i])} ${xyXUnit} → ${fmtNum(pair(u, s)[1][i])} ${d.unit}`,
        })),
      ] as uPlot.Series[],
      axes: [
        { ...axis, ...unitAxis(1), label: xyXLabel, size: 24 },
        ...units.map((u, i) => ({ ...axis, ...unitAxis(1), scale: u, label: u, side: i % 2 === 0 ? 3 : 1, size: 50 })),
      ],
      cursor: {
        drag: { x: true, y: true },
        // hover picks the point nearest the pointer on the screen
        dataIdx: (u, s) => {
          const [xs, ys] = pair(u, s);
          const { facets } = u.series[s];
          const sx = u.scales[facets![0].scale];
          const sy = u.scales[facets![1].scale];
          if (sx.min == null || sx.max == null || sy.min == null || sy.max == null) return null;
          const kx = u.over.clientWidth / (sx.max - sx.min || 1);
          const ky = u.over.clientHeight / (sy.max - sy.min || 1);
          const cx = u.posToVal(u.cursor.left!, facets![0].scale);
          const cy = u.posToVal(u.cursor.top!, facets![1].scale);
          let best: number | null = null;
          let dist = Infinity;
          for (let i = 0; i < xs.length; i++) {
            const x = xs[i];
            const y = ys[i];
            if (x == null || y == null) continue;
            const d = ((x - cx) * kx) ** 2 + ((y - cy) * ky) ** 2;
            if (d < dist) {
              dist = d;
              best = i;
            }
          }
          return best;
        },
      },
    };
  }, [xyLook, xyXLabel, xyXUnit, yLook, theme]);

  // sweep-summary data: chosen metric vs swept value across the family
  const sweepMetrics = useMemo(() => {
    const set = new Set<string>();
    for (const r of family) for (const s of r.result.summary) set.add(s.label);
    return [...set];
  }, [family]);
  // the picked metric while the family reports it, else its first headline number
  const sweepMetric = sweepMetrics.includes(sweepPick) ? sweepPick : pickSweepMetric(sweepMetrics);
  const sweepUnit = family[0]?.sweepUnit ?? "";
  const sweepParam = family[0]?.sweepParam ?? "value";
  // complete points form the curve (y); incomplete ones, when shown, are
  // separate hollow markers (yIncomplete) that the curve does not pass through
  const sweepData = useMemo(
    () =>
      (showIncomplete ? family : completeFamily)
        .map((r) => {
          const sv = r.result.summary.find((s) => s.label === sweepMetric);
          const v = sv ? sv.value : null;
          return {
            x: r.sweepValue ?? 0,
            y: r.incomplete ? null : v,
            yIncomplete: r.incomplete ? v : null,
            reason: r.incomplete ?? "",
            unit: sv?.unit ?? "",
          };
        })
        .filter((d) => d.y !== null || d.yIncomplete !== null),
    [family, completeFamily, showIncomplete, sweepMetric],
  );
  const metricUnit = sweepData[0]?.unit ?? "";
  const sweepCols = useMemo(
    (): uPlot.AlignedData => [
      sweepData.map((d) => d.x),
      sweepData.map((d) => d.y),
      ...(showIncomplete ? [sweepData.map((d) => d.yIncomplete)] : []),
    ],
    [sweepData, showIncomplete],
  );
  const sweepReasons = JSON.stringify(sweepData.map((d) => d.reason));
  const sweepOptions = useMemo((): PlotOptions => {
    const axis = axisStyle(theme);
    const bg = getComputedStyle(document.documentElement).getPropertyValue("--ss-panel").trim();
    const reasons: string[] = JSON.parse(sweepReasons);
    const val = (_u: uPlot, v: number | null) => (v == null ? "—" : `${fmtNum(v)} ${metricUnit}`);
    return {
      scales: { x: { time: false }, y: { range: yRange({}, uPlot.rangeNum) } },
      series: [
        // the swept value as the run picker names it, unrounded
        { label: sweepParam, value: (_u, v) => (v == null ? "—" : `${v}${sweepUnit ? ` ${sweepUnit}` : ""}`) },
        {
          label: sweepMetric,
          stroke: PALETTE[0],
          width: 1.8,
          spanGaps: true,
          points: { show: true, size: 6, fill: PALETTE[0] },
          value: val,
        },
        // incomplete runs: hollow markers the curve does not pass through
        ...(showIncomplete
          ? [
              {
                label: "incomplete runs (partial values)",
                stroke: PALETTE[0],
                paths: () => null,
                points: { show: true, size: 8, width: 1.5, fill: bg },
                value: (u: uPlot, v: number | null, _s: number, i: number | null) =>
                  v == null || i == null ? "—" : `${val(u, v)} (${reasons[i]})`,
              },
            ]
          : []),
      ],
      axes: [
        { ...axis, label: `${sweepParam}${sweepUnit ? ` [${sweepUnit}]` : ""}`, size: 24 },
        { ...axis, ...unitAxis(1), label: metricUnit, size: 54 },
      ],
      cursor: { drag: { x: true, y: false } },
    };
  }, [sweepParam, sweepUnit, sweepMetric, metricUnit, sweepReasons, showIncomplete, theme]);

  // on: A and B at a quarter and three quarters of the chart's view (the
  // whole run in the table and X-Y views)
  const toggleCursors = () => {
    if (!activeRun || !canMeasure) return;
    const { setCursors } = useUIStore.getState();
    if (cursorsOn) setCursors(activeRun.id, null);
    else if (view === "chart") setCursors(activeRun.id, cursorsIn(activeRun, xAxis.kind, plotRef.current?.xRange() ?? null));
    else setCursors(activeRun.id, cursorsIn(activeRun, "t", null));
  };
  // C does the same, unless it is typed into a field (a ticked checkbox is
  // fine) or a dialog is open over the page
  const onCursorKey = useEffectEvent(toggleCursors);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() !== "c" || e.ctrlKey || e.metaKey || e.altKey || e.repeat) return;
      const el = e.target as HTMLElement;
      const typing =
        el.isContentEditable ||
        el.tagName === "TEXTAREA" ||
        el.tagName === "SELECT" ||
        (el.tagName === "INPUT" && (el as HTMLInputElement).type !== "checkbox");
      const ui = useUIStore.getState();
      if (typing || ui.dialog || ui.paramDialogId) return;
      e.preventDefault();
      onCursorKey();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const toggle = (key: string) => {
    const cur = savedChannels ?? defaultKeys;
    savePlot({ channels: cur.includes(key) ? cur.filter((k) => k !== key) : [...cur, key] });
  };

  if (runs.length === 0 && runsLoading) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-[color:var(--ss-text-dim)]">
        <LineChartIcon size={36} strokeWidth={1} />
        <div className="text-[13px]">
          Loading {storedRunCount} stored run{storedRunCount > 1 ? "s" : ""}…
        </div>
      </div>
    );
  }

  if (runs.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-[color:var(--ss-text-dim)]">
        <LineChartIcon size={36} strokeWidth={1} />
        <div className="text-[13px]">No results yet — run a simulation case first.</div>
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-3"
          disabled={running}
          onClick={() => void run()}
        >
          <Play size={13} className="text-[color:var(--ss-accent)]" />
          {running ? "Running…" : `Run '${caseName ?? "active case"}'`}
        </button>
      </div>
    );
  }

  const otherRuns = runs.filter((r) => r.id !== activeRun?.id);
  // the summary's value columns: this run, the baseline (with its change),
  // then the other overlaid runs
  const columns = activeRun
    ? [
        { run: activeRun, head: multiRun ? runShort(activeRun) : baseline ? "This run" : "Value" },
        ...(baseline ? [{ run: baseline, head: "Baseline" }] : []),
        ...overlayRuns.filter((r) => r !== baseline).map((r) => ({ run: r, head: runShort(r) })),
      ]
    : [];

  return (
    <div className="flex h-full">
      {/* run + channel picker */}
      <div className="flex w-[280px] shrink-0 flex-col border-r border-[color:var(--ss-border)]">
        {/* (it scrolls as a whole when Run info and what changed leave the
            channel list too little room, as at a large interface size) */}
        <div className="flex min-h-0 flex-col gap-1.5 overflow-y-auto border-b [&>*]:shrink-0 border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] p-1.5">
          <div className="flex items-center gap-1">
            <select
              className="ss-input min-w-0 flex-1"
              aria-label="Primary run"
              value={activeRun?.id ?? ""}
              onChange={(e) => setActiveRun(e.target.value)}
              title={`${activeRun ? `${runTitle(activeRun)}${activeRun.incomplete || activeRun.status !== "success" ? "" : " · success"} — ` : ""}primary run (drives the channel list, table and summary)`}
            >
              {runs.map((r) => (
                <option key={r.id} value={r.id} title={runTitle(r)}>
                  {runLabel(r)}
                </option>
              ))}
            </select>
            <button
              className="ss-toolbtn"
              title="Delete this run (also from disk)"
              disabled={!activeRun || running}
              onClick={() => {
                if (!activeRun) return;
                const id = activeRun.id;
                void confirmDialog({
                  title: "Delete this run?",
                  message: `This deletes '${runLabel(activeRun)}' from disk. It cannot be recovered.`,
                  confirmLabel: "Delete run",
                  danger: true,
                }).then((ok) => {
                  if (ok) void removeRun(id);
                });
              }}
            >
              <X size={13} />
            </button>
            <button
              className={`ss-toolbtn ${showRunInfo ? "bg-[color:var(--ss-active)]" : ""}`}
              title="Run info: the model, settings and version this run was made with"
              aria-pressed={showRunInfo}
              disabled={!activeRun}
              onClick={() => setShowRunInfo(!showRunInfo)}
            >
              <Info size={13} />
            </button>
          </div>
          {showRunInfo && activeRun && <RunInfo run={activeRun} />}

          {/* the run the numbers are compared with, and what changed since (RES-10) */}
          {otherRuns.length > 0 && activeRun && (
            <>
              <div className="flex items-center gap-1">
                <span className="shrink-0 text-[10px] text-[color:var(--ss-text-dim)]">Baseline</span>
                <select
                  className="ss-input min-w-0 flex-1"
                  aria-label="Baseline run"
                  value={pick === null ? "none" : (picked?.id ?? "")}
                  onChange={(e) =>
                    savePlot({ baselineRunId: e.target.value === "" ? undefined : e.target.value === "none" ? null : e.target.value })
                  }
                  title={`${baseline ? `${runTitle(baseline)} — ` : ""}the run the summary and headline numbers are compared with`}
                >
                  <option value="">Previous run of this case ({previous ? runShort(previous) : "none yet"})</option>
                  <option value="none">None</option>
                  {otherRuns.map((r) => (
                    <option key={r.id} value={r.id} title={runTitle(r)}>
                      {runLabel(r)}
                    </option>
                  ))}
                </select>
              </div>
              {baseline && (
                <>
                  <label
                    className="flex cursor-pointer items-center gap-1.5 text-[11px]"
                    title="Draw the baseline run's lines on the chart, dashed and faint, with this run's"
                  >
                    <input
                      type="checkbox"
                      checked={comparePrevious}
                      onChange={(e) => useUIStore.getState().setComparePrevious(projectId, e.target.checked)}
                    />
                    Draw the baseline faint on the chart
                  </label>
                  <RunChanges run={activeRun} base={baseline} />
                </>
              )}
            </>
          )}

          {/* overlay set */}
          <div className="rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)]">
            <div className="flex items-center gap-1 px-1.5 py-1 text-[10px] text-[color:var(--ss-text-dim)]">
              <Layers size={11} /> Overlay ({overlayRuns.length})
              {family.length >= 2 && (
                <button
                  className="ml-auto rounded px-1 hover:bg-[color:var(--ss-hover)]"
                  title={
                    incompleteCount > 0
                      ? "Overlay every complete run in this sweep family (stopped or failed points are left out)"
                      : "Overlay every run in this sweep family"
                  }
                  onClick={() =>
                    setOverlayRuns(completeFamily.filter((r) => r.id !== activeRun?.id).map((r) => r.id))
                  }
                >
                  Overlay family
                </button>
              )}
              {overlayRuns.length > 0 && (
                <button
                  className={`rounded px-1 hover:bg-[color:var(--ss-hover)] ${family.length >= 2 ? "" : "ml-auto"}`}
                  onClick={clearOverlays}
                >
                  Clear
                </button>
              )}
            </div>
            {otherRuns.length === 0 ? (
              <div className="px-1.5 pb-1 text-[10px] italic text-[color:var(--ss-text-dim)]">
                Only one run — overlay appears once you have more.
              </div>
            ) : (
              <div className="max-h-[104px] overflow-y-auto border-t border-[color:var(--ss-border)]">
                {otherRuns.map((r) => (
                  <label
                    key={r.id}
                    className="flex cursor-pointer items-center gap-1.5 px-1.5 py-0.5 text-[11px] hover:bg-[color:var(--ss-hover)]"
                    title={runTitle(r)}
                  >
                    <input
                      type="checkbox"
                      checked={overlayRunIds.includes(r.id)}
                      onChange={() => toggleOverlayRun(r.id)}
                    />
                    <span
                      className="h-2 w-2 shrink-0 rounded-full"
                      style={{ background: overlayRunIds.includes(r.id) ? overlayColorOf(r.id) : "#c0c6d0" }}
                    />
                    <span className="truncate">
                      {r.sweepValue !== undefined ? runShort(r) : `${r.caseName} · ${r.name ?? runTime(r)}`}
                    </span>
                  </label>
                ))}
              </div>
            )}
          </div>

          <div className="flex items-center gap-1 rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] px-1.5">
            <Search size={12} className="shrink-0 text-[color:var(--ss-text-dim)]" />
            <input
              className="w-full bg-transparent py-1 text-[12px] outline-none"
              placeholder="Search channels…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            {search && (
              <button className="text-[color:var(--ss-text-dim)] hover:text-[color:var(--ss-text)]" onClick={() => setSearch("")}>
                <X size={12} />
              </button>
            )}
          </div>
        </div>
        <div className="min-h-[132px] flex-1 overflow-y-auto py-1">
          {byElement.length === 0 && (
            <div className="px-3 py-2 text-[11px] italic text-[color:var(--ss-text-dim)]">
              No channels match “{search}”.
            </div>
          )}
          {byElement.map(([element, channels]) => (
            <div key={element}>
              <div className="px-2 py-1 text-[11px] font-semibold text-[color:var(--ss-text-dim)]">
                {element}
              </div>
              {channels.map((c) => {
                const key = channelKey(c);
                return (
                  <label
                    key={key}
                    className="ss-tree-row cursor-pointer pl-4"
                    title={`${c.label} [${c.unit}]`}
                  >
                    <input
                      type="checkbox"
                      checked={selectedKeys.has(key)}
                      onChange={() => toggle(key)}
                    />
                    <span
                      className="h-2 w-2 shrink-0 rounded-full"
                      style={{ background: selectedKeys.has(key) ? channelColor(key) : "#d0d5dd" }}
                    />
                    <span className="truncate">{c.label.split(" · ")[1] ?? c.label}</span>
                    <span className="ml-auto pr-1 text-[10px] text-[color:var(--ss-text-dim)]">
                      {c.unit}
                    </span>
                  </label>
                );
              })}
            </div>
          ))}
        </div>
      </div>

      {/* chart + summary; the chart keeps a readable height, and when the
          cursors' table and the summary leave it less (a large interface
          size), the page scrolls */}
      <div className="flex min-w-0 flex-1 flex-col overflow-y-auto">
        {/* (a container: in a narrow one, as at a large interface size, the
            last three buttons show only their icons, so the row stays one line) */}
        <div className="ss-panel-toolbar @container">
          <span className="text-[11px] text-[color:var(--ss-text-dim)]">
            {view === "sweep" ? (
              <>
                Sweep · {completeFamily.length} complete run(s)
                {incompleteCount > 0 && (
                  <span className="text-[color:var(--ss-warn)]">
                    {" "}
                    · {incompleteCount} incomplete {showIncomplete ? "shown hollow" : "not plotted"}
                  </span>
                )}
              </>
            ) : view === "xy" ? (
              <>X-Y · {xyYChannels.length} series vs {xyXShort || "—"}</>
            ) : (
              <>
                {shownKeys.length} channel(s)
                {multiRun && <span> · {plotRuns.length} runs overlaid</span>}
              </>
            )}
            {result && view !== "sweep" && (
              <>
                {" · "}
                <b
                  className={
                    running && activeRun?.status === "running"
                      ? "text-[color:var(--ss-accent)]"
                      : result.status === "success" && !activeRun?.incomplete
                        ? "text-[color:var(--ss-ok)]"
                        : result.status !== "failed"
                          ? "text-[color:var(--ss-warn)]"
                          : "text-[color:var(--ss-err)]"
                  }
                >
                  {activeRun?.status === "running"
                    ? "running…"
                    : activeRun?.incomplete
                      ? `incomplete (${activeRun.incomplete})`
                      : result.status}
                </b>
              </>
            )}
          </span>
          <div className="ml-auto flex items-center gap-1">
            {view === "sweep" && incompleteCount > 0 && (
              <label
                className="flex items-center gap-1 text-[11px] text-[color:var(--ss-text-dim)]"
                title="Show stopped or failed sweep points as hollow markers (their numbers are partial)"
              >
                <input
                  type="checkbox"
                  checked={showIncomplete}
                  onChange={(e) => setShowIncomplete(e.target.checked)}
                />
                Show incomplete ({incompleteCount})
              </label>
            )}
            {view === "sweep" && sweepMetrics.length > 0 && (
              <select
                className="ss-input max-w-[190px] py-0.5 text-[11px]"
                aria-label="Sweep metric"
                value={sweepMetric}
                onChange={(e) => savePlot({ sweepMetric: e.target.value })}
                title="Summary metric to plot against the swept value"
              >
                {sweepMetrics.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            )}
            {view === "xy" && shownKeys.length > 0 && (
              <select
                className="ss-input max-w-[200px] py-0.5 text-[11px]"
                value={xyXKey}
                onChange={(e) => savePlot({ xKey: e.target.value })}
                title="Channel to plot on the X axis (the other ticked channels become Y series)"
              >
                {shownKeys.map((k) => {
                  const c = channelByKey.get(k);
                  const short = c ? (c.label.split(" · ")[1] ?? c.label) : k;
                  return (
                    <option key={k} value={k}>
                      X: {short}
                      {c ? ` [${c.unit}]` : ""}
                    </option>
                  );
                })}
              </select>
            )}
            {view === "chart" && (
              <select
                className="ss-input py-0.5 text-[11px]"
                aria-label="X axis"
                value={xMode}
                // (a zoom in s is no zoom in m: it is dropped)
                onChange={(e) =>
                  savePlot({
                    xAxis: e.target.value as XAxisMode,
                    ...((e.target.value === "distance") !== (xAxis.kind === "distance") ? { zoom: undefined } : {}),
                  })
                }
                title="Plot against time or the distance driven"
              >
                <option value="auto">Time · auto</option>
                <option value="s">Time [s]</option>
                <option value="min">Time [min]</option>
                <option value="h">Time [h]</option>
                <option value="distance" disabled={!canDistance}>
                  {canDistance ? "Distance" : "Distance (a run has no Vehicle)"}
                </option>
              </select>
            )}
            {(view === "chart" || view === "xy") && (
              <AxesMenu
                units={[...new Set(view === "chart" ? seriesDefs.map((d) => d.unit) : xyYChannels.map((c) => c.unit))]}
                value={yAxes}
                onChange={(v) => savePlot({ yAxes: v })}
              />
            )}
            <div className="flex overflow-hidden rounded border border-[color:var(--ss-border)]">
              <button
                className={`flex items-center gap-1 whitespace-nowrap px-2 py-1 text-[11px] ${
                  view === "chart" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                aria-pressed={view === "chart"}
                onClick={() => setView("chart")}
                title="Time-series chart"
              >
                <LineChartIcon size={12} /> Chart
              </button>
              <button
                className={`flex items-center gap-1 whitespace-nowrap border-l border-[color:var(--ss-border)] px-2 py-1 text-[11px] ${
                  view === "table" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                aria-pressed={view === "table"}
                onClick={() => setView("table")}
                title="Table view"
              >
                <Table2 size={12} /> Table
              </button>
              <button
                className={`flex items-center gap-1 whitespace-nowrap border-l border-[color:var(--ss-border)] px-2 py-1 text-[11px] disabled:opacity-40 ${
                  view === "xy" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                aria-pressed={view === "xy"}
                onClick={() => setView("xy")}
                disabled={shownKeys.length < 2}
                title={
                  shownKeys.length < 2
                    ? "Tick at least two channels to plot one against another"
                    : "X-Y plot: one channel against another (e.g. torque vs. speed)"
                }
              >
                <ChartScatter size={12} /> X-Y
              </button>
              <button
                className={`flex items-center gap-1 whitespace-nowrap border-l border-[color:var(--ss-border)] px-2 py-1 text-[11px] disabled:opacity-40 ${
                  view === "sweep" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                aria-pressed={view === "sweep"}
                onClick={() => setView("sweep")}
                disabled={family.length < 2}
                title={family.length < 2 ? "Run a parameter sweep to enable this" : "Metric vs. swept value"}
              >
                <TrendingUp size={12} /> Sweep
              </button>
              <button
                className={`flex items-center gap-1 whitespace-nowrap border-l border-[color:var(--ss-border)] px-2 py-1 text-[11px] ${
                  view === "energy" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                aria-pressed={view === "energy"}
                onClick={() => setView("energy")}
                title="Energy: where the battery's or fuel's energy went, as a Sankey chart and per part"
              >
                <Zap size={12} /> <span className="@max-[880px]:sr-only">Energy</span>
              </button>
              <button
                className={`flex items-center gap-1 whitespace-nowrap border-l border-[color:var(--ss-border)] px-2 py-1 text-[11px] ${
                  view === "duty" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                aria-pressed={view === "duty"}
                onClick={() => setView("duty")}
                title="Duty: each motor's, battery's and engine's highest, mean and RMS power, torque and current"
              >
                <Gauge size={12} /> <span className="@max-[880px]:sr-only">Duty</span>
              </button>
            </div>
            <button
              className={`ss-toolbtn border border-[color:var(--ss-border)] ${cursorsOn ? "bg-[color:var(--ss-active)]" : ""}`}
              aria-pressed={cursorsOn}
              aria-keyshortcuts="C"
              disabled={!canMeasure}
              title="Measurement cursors A and B (C)"
              onClick={toggleCursors}
            >
              <SquareSplitHorizontal size={12} /> <span className="@max-[880px]:sr-only">Cursors</span>
            </button>
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)]"
              disabled={view === "table" || report}
              title="Export the chart as a PNG image"
              onClick={() =>
                plotRef.current?.png(
                  getComputedStyle(document.documentElement).getPropertyValue("--ss-panel").trim(),
                  `lightsim-${activeRun?.caseName ?? "chart"}`,
                )
              }
            >
              <ImageIcon size={12} /> <span className="@max-[880px]:sr-only">PNG</span>
            </button>
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)]"
              disabled={!result || selectedKeys.size === 0}
              title="Export the primary run's ticked channels as CSV"
              // t_s as before unless an x axis was picked for the chart shown
              onClick={() =>
                activeRun &&
                exportCsv(
                  activeRun,
                  selectedKeys,
                  `lightsim-${activeRun.caseName}`,
                  view === "chart" && xMode !== "auto" ? xAxis : xAxisFor("s", []),
                )
              }
            >
              <Download size={12} /> <span className="@max-[880px]:sr-only">CSV</span>
            </button>
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)]"
              disabled={!result || !activeRun || activeRun.status === "running"}
              title="Export the primary run for MATLAB or Python (.mat): every channel with its unit, and the run's details"
              onClick={() => activeRun && void exportMat(activeRun)}
            >
              <Download size={12} /> <span className="@max-[880px]:sr-only">MATLAB</span>
            </button>
          </div>
        </div>

        {/* the model has changed since this run (UX-41) */}
        {activeRun && <StaleBanner run={activeRun} />}

        {/* the run's headline numbers, each with its marks, in view at once */}
        {headline.length > 0 && view !== "sweep" && (
          <div className="flex shrink-0 items-center gap-1 border-b border-[color:var(--ss-border)] p-1">
            <dl aria-label="Headline results" className="m-0 flex min-w-0 flex-1 flex-wrap gap-1">
              {headline.map((s, i) => {
                const ch = baseline && changeOf(s, baseline.result.summary.find((b) => sameFigure(b, s)));
                return (
                  <div
                    key={i}
                    className="min-w-[128px] flex-1 rounded border border-[color:var(--ss-border)] px-2 py-0.5"
                    title={s.label}
                  >
                    <dt className="truncate text-[10px] text-[color:var(--ss-text-dim)]">{s.label}</dt>
                    <dd className="m-0">
                      <span className="font-mono text-[15px] font-semibold">{s.value.toLocaleString()}</span>{" "}
                      <span className="text-[11px] text-[color:var(--ss-text-dim)]">{s.unit}</span>
                      <SummaryMark sv={s} why />
                      {ch && (
                        <div
                          className={`truncate text-[10px] ${changeClass(ch)}`}
                          title={
                            ch.noise
                              ? NOISE
                              : `${ch.diff} (${ch.pct}) against the baseline, ${runTitle(baseline)}`
                          }
                        >
                          {ch.noise ? "~ 0" : `${ch.diff} (${ch.pct})`} vs baseline
                        </div>
                      )}
                    </dd>
                  </div>
                );
              })}
            </dl>
            {estimate && (
              <span className="shrink-0 px-1 text-[10px] text-[color:var(--ss-text-dim)]" title={ESTIMATE}>
                estimates
              </span>
            )}
          </div>
        )}

        {/* expected values and hand calculations (VAL-35) */}
        {result?.references && result.references.length > 0 && view !== "sweep" && view !== "energy" && view !== "duty" && (
          <div className="max-h-[96px] shrink-0 overflow-y-auto border-b border-[color:var(--ss-border)] px-2 py-0.5">
            <ReferenceList checks={result.references} compact />
          </div>
        )}

        {view === "energy" && activeRun ? (
          <EnergyView run={activeRun} />
        ) : view === "duty" && activeRun ? (
          <DutyView run={activeRun} />
        ) : view === "table" ? (
          <div className="min-h-[260px] flex-[3] overflow-auto" onScroll={onTableScroll}>
            {activeChannels.length > 0 && tableData.length > 0 ? (
              <table className="w-full border-collapse" aria-rowcount={tableData.length + 1}>
                <thead className="sticky top-0 z-10">
                  <tr style={{ height: TABLE_ROW_H }}>
                    <th className="ss-th w-[70px] text-right">t [s]</th>
                    {activeChannels.map((c) => (
                      <th key={channelKey(c)} className="ss-th text-right" title={c.label}>
                        {c.label.split(" · ")[1] ?? c.label} [{c.unit}]
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {tableFirst > 0 && <tr aria-hidden style={{ height: tableFirst * TABLE_ROW_H }} />}
                  {tableData.slice(tableFirst, tableLast).map((row, j) => (
                    <tr
                      key={tableFirst + j}
                      aria-rowindex={tableFirst + j + 2}
                      style={{ height: TABLE_ROW_H }}
                      className="hover:bg-[color:var(--ss-hover)]"
                    >
                      <td className="ss-td whitespace-nowrap text-right font-mono text-[color:var(--ss-text-dim)]">
                        {typeof row.t === "number" ? row.t.toLocaleString() : row.t}
                      </td>
                      {activeChannels.map((c) => {
                        const v = row[channelKey(c)];
                        return (
                          <td key={channelKey(c)} className="ss-td whitespace-nowrap text-right font-mono">
                            {typeof v === "number"
                              ? v.toLocaleString(undefined, { maximumFractionDigits: 4 })
                              : "—"}
                          </td>
                        );
                      })}
                    </tr>
                  ))}
                  {tableLast < tableData.length && (
                    <tr aria-hidden style={{ height: (tableData.length - tableLast) * TABLE_ROW_H }} />
                  )}
                </tbody>
              </table>
            ) : (
              <div className="flex h-full items-center justify-center text-[12px] text-[color:var(--ss-text-dim)]">
                Tick channels on the left to tabulate them.
              </div>
            )}
          </div>
        ) : view === "sweep" ? (
          <div className="min-h-[260px] flex-[3] p-1" ref={chartHost}>
            {sweepData.length > 0 && hasSize ? (
              <Plot key="sweep" options={sweepOptions} data={sweepCols} label="Sweep chart" ref={plotRef} />
            ) : (
              <div className="flex h-full items-center justify-center text-[12px] text-[color:var(--ss-text-dim)]">
                {family.length < 2
                  ? "Run a parameter sweep to see metric-vs-value here."
                  : "This metric has no values across the family."}
              </div>
            )}
          </div>
        ) : view === "xy" ? (
          <div className="min-h-[260px] flex-[3] p-1" ref={chartHost}>
            {xyData.length > 1 && hasSize ? (
              <Plot key="xy" options={xyOptions} data={xyData} label="X-Y chart" ref={plotRef} />
            ) : (
              <div className="flex h-full items-center justify-center px-4 text-center text-[12px] text-[color:var(--ss-text-dim)]">
                {shownKeys.length < 2
                  ? "Tick at least two channels on the left — one becomes the X axis, the rest are plotted against it."
                  : "No samples to plot for this pair."}
              </div>
            )}
          </div>
        ) : (
          <div className="min-h-[260px] flex-[3] p-1" ref={chartHost}>
            {seriesDefs.length > 0 && hasSize ? (
              // each case opens on the zoom it kept (RES-19); a change
              // between time and distance starts from the whole run
              <Plot
                key={`chart-${selKey}-${xAxis.kind}`}
                options={chartOptions}
                data={chartData}
                label="Results chart"
                ref={plotRef}
                xRange={plot?.zoom?.kind === xAxis.kind ? plot.zoom : null}
                onXRangeChange={(r) => savePlot({ zoom: r ? { kind: xAxis.kind, ...r } : undefined })}
              />
            ) : (
              <div className="flex h-full items-center justify-center text-[12px] text-[color:var(--ss-text-dim)]">
                Tick channels on the left to plot them.
              </div>
            )}
          </div>
        )}

        {view === "chart" && activeRun && <LimitLegend run={activeRun} onTime={xAxis.kind === "t"} />}

        {cursorsOn && canMeasure && activeRun && (
          <MeasurePanel run={activeRun} series={seriesDefs} kind={view === "chart" ? xAxis.kind : "t"} />
        )}

        {/* every summary value, one click away with one run; opened by
            overlays, whose runs it sets side by side (hidden, not dropped, in
            the sweep view, so it comes back as the user left it) */}
        {result && result.summary.length > 0 && (
          <details
            hidden={view === "sweep"}
            open={multiRun}
            className="shrink-0 border-t border-[color:var(--ss-border)]"
          >
            <summary className="cursor-pointer px-2 py-0.5 text-[11px] text-[color:var(--ss-text-dim)]">
              All summary values ({result.summary.length})
            </summary>
            <div className="max-h-[130px] overflow-auto" tabIndex={0} role="region" aria-label="Summary">
              <table className="w-full border-collapse">
                <thead className="sticky top-0">
                  <tr>
                    {estimate ? (
                      <th className="ss-th" title={ESTIMATE}>
                        Summary value · estimate
                      </th>
                    ) : (
                      <th className="ss-th">
                        Summary value{" "}
                        <button
                          className="text-[color:var(--ss-accent)] underline"
                          title="What each summary value means and how it is worked out"
                          onClick={() => openHelp("reference/results.html")}
                        >
                          what they mean
                        </button>
                      </th>
                    )}
                    {columns.map(({ run: r, head }) => (
                      <Fragment key={r.id}>
                        <th className="ss-th w-[110px] text-right" title={runTitle(r)}>
                          {multiRun && plotRuns.includes(r) && (
                            <span
                              className="mr-1 inline-block h-2 w-2 rounded-full align-middle"
                              style={{ background: runColor(plotRuns.indexOf(r)) }}
                              aria-hidden="true"
                            />
                          )}
                          {head}
                        </th>
                        {r === baseline && (
                          <>
                            <th className="ss-th w-[80px] whitespace-nowrap text-right" title="This run's value less the baseline's">
                              Change
                            </th>
                            <th className="ss-th w-[80px] whitespace-nowrap text-right" title="The change as a share of the baseline's value">
                              % change
                            </th>
                          </>
                        )}
                      </Fragment>
                    ))}
                    <th className="ss-th w-[56px]">Unit</th>
                  </tr>
                </thead>
                <tbody>
                  {result.summary.map((s, i) => (
                    <tr key={i} className="hover:bg-[color:var(--ss-hover)]">
                      <td className="ss-td" title={define(s.label)}>
                        {s.label}
                        {/* the reason under the label, where there is room; the
                            value cells keep a short marker (reason in its tooltip) */}
                        {s.notValid && (
                          <div className="text-[10px] text-[color:var(--ss-warn)]">not valid: {s.notValid}</div>
                        )}
                      </td>
                      {columns.map(({ run: r }, ci) => {
                        const sv = r.result.summary.find((x) => sameFigure(x, s));
                        const ch = r === baseline ? changeOf(s, sv) : null;
                        return (
                          <Fragment key={r.id}>
                            <td className={`ss-td text-right font-mono ${ci > 0 ? "text-[color:var(--ss-text-dim)]" : ""}`}>
                              {sv ? sv.value.toLocaleString() : "—"}
                              <SummaryMark sv={sv} />
                            </td>
                            {r === baseline && (
                              <>
                                <td className={`ss-td whitespace-nowrap text-right font-mono ${changeClass(ch)}`} title={ch?.noise ? NOISE : undefined}>
                                  {ch?.diff ?? "—"}
                                </td>
                                <td className={`ss-td whitespace-nowrap text-right font-mono ${changeClass(ch)}`} title={ch?.noise ? NOISE : undefined}>
                                  {ch?.pct ?? "—"}
                                </td>
                              </>
                            )}
                          </Fragment>
                        );
                      })}
                      <td className="ss-td text-[color:var(--ss-text-dim)]">{s.unit}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </details>
        )}
      </div>
    </div>
  );
}
