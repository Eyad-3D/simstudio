import { useEffect, useMemo, useState } from "react";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  ChartScatter,
  Download,
  Image as ImageIcon,
  Info,
  Layers,
  LineChart as LineChartIcon,
  Play,
  Search,
  Table2,
  TrendingUp,
  X,
} from "lucide-react";
import { confirmDialog } from "../../dialog";
import { useActiveRun, useOverlayRuns, useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import type { Channel, SimResult, SimRun } from "../../types";
import { PALETTE, channelKey, minMaxIndices, useHasSize } from "./chartUtils";
import { RunInfo } from "./RunInfo";

// dash patterns to distinguish channels when several runs are overlaid at once
const DASHES = ["", "5 3", "2 2", "7 3 2 3", "9 4"];

// Table view rows have a fixed height, so only the rows in view are drawn
const TABLE_ROW_H = 26; // px
const TABLE_PAGE_ROWS = 80; // drawn before the first scroll event
const TABLE_OVERSCAN = 10;

function runTime(r: SimRun): string {
  return new Date(r.startedAt).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function runLabel(r: SimRun): string {
  return `${r.caseName} · ${runTime(r)} · ${r.incomplete ? `incomplete (${r.incomplete})` : r.status}`;
}

/** Compact run label for legends/overlay chips — swept value if present. */
function runShort(r: SimRun): string {
  const mark = r.incomplete ? " (incomplete)" : "";
  if (r.sweepValue !== undefined)
    return `${r.sweepValue}${r.sweepUnit ? ` ${r.sweepUnit}` : ""}${mark}`;
  return `${runTime(r)}${mark}`;
}

function exportCsv(result: SimResult, keys: Set<string>, name: string) {
  const channels = result.channels.filter((c) => keys.has(channelKey(c)));
  if (channels.length === 0) return;
  const header = ["t_s", ...channels.map((c) => `${c.label} [${c.unit}]`)];
  const rows = channels[0].timeSeries.map((pt, i) => [
    pt.t,
    ...channels.map((c) => c.timeSeries[i]?.value ?? ""),
  ]);
  const csv = [header, ...rows].map((r) => r.join(",")).join("\n");
  const blob = new Blob([csv], { type: "text/csv" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${name}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

/** Rasterize the chart's SVG to a PNG download (2× for crispness). */
function exportPng(host: HTMLElement | null, bg: string, name: string) {
  const svg = host?.querySelector("svg");
  if (!svg) return;
  const rect = svg.getBoundingClientRect();
  const clone = svg.cloneNode(true) as SVGElement;
  clone.setAttribute("width", String(rect.width));
  clone.setAttribute("height", String(rect.height));
  const xml = new XMLSerializer().serializeToString(clone);
  const url = URL.createObjectURL(new Blob([xml], { type: "image/svg+xml;charset=utf-8" }));
  const img = new Image();
  img.onload = () => {
    const scale = 2;
    const canvas = document.createElement("canvas");
    canvas.width = rect.width * scale;
    canvas.height = rect.height * scale;
    const ctx = canvas.getContext("2d");
    if (ctx) {
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.scale(scale, scale);
      ctx.drawImage(img, 0, 0);
    }
    URL.revokeObjectURL(url);
    canvas.toBlob((blob) => {
      if (!blob) return;
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = `${name}.png`;
      a.click();
      URL.revokeObjectURL(a.href);
    }, "image/png");
  };
  img.src = url;
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
  const theme = useUIStore((s) => s.theme);

  const result = activeRun?.result;
  const selKey = activeRun?.caseId ?? "";

  const [selected, setSelected] = useState<Record<string, string[]>>({});
  const [view, setView] = useState<"chart" | "table" | "sweep" | "xy">("chart");
  const [search, setSearch] = useState("");
  const [sweepMetric, setSweepMetric] = useState("");
  const [xyXKey, setXyXKey] = useState(""); // channel used as the X axis in the X-Y view
  const [showRunInfo, setShowRunInfo] = useState(false);
  const { ref: chartHost, hasSize, element: chartEl } = useHasSize<HTMLDivElement>();

  // sensible default channel selection the first time a case's results are shown
  // (wait for channels: a live run starts with none, and an empty pick would
  // stick for the rest of the session)
  useEffect(() => {
    if (!result || !selKey || selected[selKey] || result.channels.length === 0) return;
    const defaults = result.channels
      .filter((c) => c.portId === "sig_soc" || (c.portId === "sig_power" && c.label.includes("Battery")))
      .slice(0, 4)
      .map(channelKey);
    setSelected((s) => ({
      ...s,
      [selKey]: defaults.length > 0 ? defaults : result.channels.slice(0, 2).map(channelKey),
    }));
  }, [result, selKey, selected]);

  const selectedKeys = useMemo(
    () => new Set(selKey ? (selected[selKey] ?? []) : []),
    [selected, selKey],
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
  const runColor = (i: number) => PALETTE[i % PALETTE.length];
  const channelColor = (key: string) => PALETTE[Math.max(0, selectedList.indexOf(key)) % PALETTE.length];
  const overlayColorOf = (id: string) => {
    const idx = overlayRunIds.indexOf(id);
    return idx >= 0 ? runColor(idx + 1) : "#c0c6d0";
  };

  // one plotted series per (run × selected channel)
  const seriesDefs = useMemo(() => {
    const defs: {
      dataKey: string;
      unit: string;
      color: string;
      dash?: string;
      legend: string;
      channel: Channel;
    }[] = [];
    plotRuns.forEach((r, ri) => {
      for (const c of r.result.channels) {
        const key = channelKey(c);
        if (!selectedKeys.has(key)) continue;
        const ci = selectedList.indexOf(key);
        const chLabel = c.label.split(" · ")[1] ?? c.label;
        defs.push({
          dataKey: `${r.id}::${key}`,
          unit: c.unit,
          color: multiRun ? runColor(ri) : channelColor(key),
          dash: multiRun && selectedList.length > 1 ? DASHES[ci % DASHES.length] : undefined,
          legend: multiRun ? `${runShort(r)} · ${chLabel}` : chLabel,
          channel: c,
        });
      }
    });
    return defs;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plotRuns, selectedKeys, selectedList, multiRun]);

  const defByKey = useMemo(
    () => new Map(seriesDefs.map((d) => [d.dataKey, d])),
    [seriesDefs],
  );
  const units = useMemo(() => [...new Set(seriesDefs.map((d) => d.unit))], [seriesDefs]);

  const chartData = useMemo(() => {
    // series on the same time grid (the channels of a run, and overlaid runs
    // of the same case) are thinned together so their rows stay aligned
    const grids = new Map<string, typeof seriesDefs>();
    for (const d of seriesDefs) {
      const ts = d.channel.timeSeries;
      const grid = `${ts.length}:${ts[0]?.t}:${ts[ts.length - 1]?.t}`;
      grids.set(grid, [...(grids.get(grid) ?? []), d]);
    }
    const map = new Map<number, Record<string, number | null>>();
    for (const defs of grids.values()) {
      for (const i of minMaxIndices(defs.map((d) => d.channel.timeSeries))) {
        for (const d of defs) {
          const pt = d.channel.timeSeries[i];
          let row = map.get(pt.t);
          if (!row) {
            row = { t: pt.t };
            map.set(pt.t, row);
          }
          row[d.dataKey] = pt.value;
        }
      }
    }
    return [...map.values()].sort((a, b) => (a.t ?? 0) - (b.t ?? 0));
  }, [seriesDefs]);

  // table shows only the active run (aligned time grid), every sample; only
  // the rows scrolled into view are drawn (see onTableScroll)
  const activeChannels = useMemo(
    () => result?.channels.filter((c) => selectedKeys.has(channelKey(c))) ?? [],
    [result, selectedKeys],
  );
  const tableData = useMemo(() => {
    if (activeChannels.length === 0) return [];
    return activeChannels[0].timeSeries.map((pt, i) => {
      const row: Record<string, number | null> = { t: pt.t };
      activeChannels.forEach((c) => {
        const p = c.timeSeries[i];
        if (p) row[channelKey(c)] = p.value;
      });
      return row;
    });
  }, [activeChannels]);
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

  // keep the chosen X channel valid as the selection changes
  useEffect(() => {
    if (selectedList.length === 0) {
      if (xyXKey) setXyXKey("");
    } else if (!selectedList.includes(xyXKey)) {
      setXyXKey(selectedList[0]);
    }
  }, [selectedList, xyXKey]);

  const xyXChannel = xyXKey ? (channelByKey.get(xyXKey) ?? null) : null;
  const xyYChannels = useMemo(
    () => activeChannels.filter((c) => channelKey(c) !== xyXKey),
    [activeChannels, xyXKey],
  );
  const xyYUnits = useMemo(() => [...new Set(xyYChannels.map((c) => c.unit))], [xyYChannels]);
  const xyXShort = xyXChannel ? (xyXChannel.label.split(" · ")[1] ?? xyXChannel.label) : "";
  const xyData = useMemo(() => {
    if (!xyXChannel || xyYChannels.length === 0) return [];
    // the same sample indices for X and every Y, so each point is a real pair
    const xs = xyXChannel.timeSeries;
    const idx = minMaxIndices([xs, ...xyYChannels.map((c) => c.timeSeries)]);
    return idx.map((i) => {
      const row: Record<string, number | null> = { x: xs[i].value };
      xyYChannels.forEach((c) => {
        row[channelKey(c)] = c.timeSeries[i].value;
      });
      return row;
    });
  }, [xyXChannel, xyYChannels]);

  // sweep-summary data: chosen metric vs swept value across the family
  const sweepMetrics = useMemo(() => {
    const set = new Set<string>();
    for (const r of family) for (const s of r.result.summary) set.add(s.label);
    return [...set];
  }, [family]);
  useEffect(() => {
    if (sweepMetrics.length === 0) return;
    if (!sweepMetrics.includes(sweepMetric)) {
      const preferred =
        sweepMetrics.find((m) => /consumption|final soc|fuel/i.test(m)) ?? sweepMetrics[0];
      setSweepMetric(preferred);
    }
  }, [sweepMetrics, sweepMetric]);
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

  const toggle = (key: string) => {
    if (!selKey) return;
    setSelected((s) => {
      const cur = s[selKey] ?? [];
      return {
        ...s,
        [selKey]: cur.includes(key) ? cur.filter((k) => k !== key) : [...cur, key],
      };
    });
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
          {running ? "Running…" : "Run active case"}
        </button>
      </div>
    );
  }

  const otherRuns = runs.filter((r) => r.id !== activeRun?.id);

  return (
    <div className="flex h-full">
      {/* run + channel picker */}
      <div className="flex w-[280px] shrink-0 flex-col border-r border-[color:var(--ss-border)]">
        <div className="flex flex-col gap-1.5 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] p-1.5">
          <div className="flex items-center gap-1">
            <select
              className="ss-input min-w-0 flex-1"
              value={activeRun?.id ?? ""}
              onChange={(e) => setActiveRun(e.target.value)}
              title="Primary run (drives the channel list, table and summary)"
            >
              {runs.map((r) => (
                <option key={r.id} value={r.id}>
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
                    title={runLabel(r)}
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
                      {r.sweepValue !== undefined ? runShort(r) : `${r.caseName} · ${runTime(r)}`}
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
        <div className="min-h-0 flex-1 overflow-y-auto py-1">
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

      {/* chart + summary */}
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="ss-panel-toolbar">
          <span className="text-[11px] text-[color:var(--ss-text-dim)]">
            {view === "sweep" ? (
              <>
                Sweep · {completeFamily.length} complete run(s)
                {incompleteCount > 0 && (
                  <span className="text-amber-600">
                    {" "}
                    · {incompleteCount} incomplete {showIncomplete ? "shown hollow" : "not plotted"}
                  </span>
                )}
              </>
            ) : view === "xy" ? (
              <>X-Y · {xyYChannels.length} series vs {xyXShort || "—"}</>
            ) : (
              <>
                {selectedKeys.size} channel(s)
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
                        ? "text-emerald-700"
                        : result.status !== "failed"
                          ? "text-amber-600"
                          : "text-red-600"
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
                value={sweepMetric}
                onChange={(e) => setSweepMetric(e.target.value)}
                title="Summary metric to plot against the swept value"
              >
                {sweepMetrics.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            )}
            {view === "xy" && selectedList.length > 0 && (
              <select
                className="ss-input max-w-[200px] py-0.5 text-[11px]"
                value={xyXKey}
                onChange={(e) => setXyXKey(e.target.value)}
                title="Channel to plot on the X axis (the other ticked channels become Y series)"
              >
                {selectedList.map((k) => {
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
            <div className="flex overflow-hidden rounded border border-[color:var(--ss-border)]">
              <button
                className={`flex items-center gap-1 px-2 py-1 text-[11px] ${
                  view === "chart" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                onClick={() => setView("chart")}
                title="Time-series chart"
              >
                <LineChartIcon size={12} /> Chart
              </button>
              <button
                className={`flex items-center gap-1 border-l border-[color:var(--ss-border)] px-2 py-1 text-[11px] ${
                  view === "table" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                onClick={() => setView("table")}
                title="Table view"
              >
                <Table2 size={12} /> Table
              </button>
              <button
                className={`flex items-center gap-1 border-l border-[color:var(--ss-border)] px-2 py-1 text-[11px] disabled:opacity-40 ${
                  view === "xy" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                onClick={() => setView("xy")}
                disabled={selectedKeys.size < 2}
                title={
                  selectedKeys.size < 2
                    ? "Tick at least two channels to plot one against another"
                    : "X-Y plot: one channel against another (e.g. torque vs. speed)"
                }
              >
                <ChartScatter size={12} /> X-Y
              </button>
              <button
                className={`flex items-center gap-1 border-l border-[color:var(--ss-border)] px-2 py-1 text-[11px] disabled:opacity-40 ${
                  view === "sweep" ? "bg-[color:var(--ss-active)] font-semibold" : "hover:bg-[color:var(--ss-hover)]"
                }`}
                onClick={() => setView("sweep")}
                disabled={family.length < 2}
                title={family.length < 2 ? "Run a parameter sweep to enable this" : "Metric vs. swept value"}
              >
                <TrendingUp size={12} /> Sweep
              </button>
            </div>
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)]"
              disabled={view === "table"}
              title="Export the chart as a PNG image"
              onClick={() =>
                exportPng(
                  chartEl.current,
                  theme === "dark" ? "#1b1f26" : "#ffffff",
                  `lightsim-${activeRun?.caseName ?? "chart"}`,
                )
              }
            >
              <ImageIcon size={12} /> PNG
            </button>
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)]"
              disabled={!result || selectedKeys.size === 0}
              onClick={() => result && exportCsv(result, selectedKeys, `lightsim-${activeRun?.caseName ?? "results"}`)}
            >
              <Download size={12} /> CSV
            </button>
          </div>
        </div>

        {view === "table" ? (
          <div className="min-h-0 flex-[3] overflow-auto" onScroll={onTableScroll}>
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
          <div className="min-h-0 flex-[3] p-1" ref={chartHost}>
            {sweepData.length > 0 && hasSize ? (
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={sweepData} margin={{ top: 10, right: 20, bottom: 16, left: 8 }}>
                  <CartesianGrid stroke={theme === "dark" ? "#2a2f37" : "#eceff3"} />
                  <XAxis
                    dataKey="x"
                    type="number"
                    domain={["dataMin", "dataMax"]}
                    tick={{ fontSize: 10 }}
                    label={{
                      value: `${sweepParam}${sweepUnit ? ` [${sweepUnit}]` : ""}`,
                      position: "insideBottom",
                      offset: -8,
                      fontSize: 11,
                    }}
                  />
                  <YAxis
                    tick={{ fontSize: 10 }}
                    width={54}
                    label={{
                      value: metricUnit,
                      angle: -90,
                      position: "insideLeft",
                      fontSize: 10,
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
                    labelFormatter={(x) => `${sweepParam} = ${x}${sweepUnit ? ` ${sweepUnit}` : ""}`}
                    formatter={(value, name, item) => [
                      `${typeof value === "number" ? value.toLocaleString(undefined, { maximumFractionDigits: 4 }) : value} ${metricUnit}` +
                        (name === "yIncomplete" ? ` — incomplete run (${item.payload.reason}), partial value` : ""),
                      sweepMetric,
                    ]}
                  />
                  <Line
                    dataKey="y"
                    type="monotone"
                    stroke={PALETTE[0]}
                    strokeWidth={1.8}
                    dot={{ r: 3, fill: PALETTE[0] }}
                    connectNulls
                    isAnimationActive={false}
                  />
                  {showIncomplete && (
                    <Line
                      dataKey="yIncomplete"
                      stroke="none"
                      dot={{ r: 4, fill: "none", stroke: PALETTE[0], strokeWidth: 1.5 }}
                      activeDot={{ r: 5, fill: "none", stroke: PALETTE[0], strokeWidth: 1.5 }}
                      isAnimationActive={false}
                    />
                  )}
                </LineChart>
              </ResponsiveContainer>
            ) : (
              <div className="flex h-full items-center justify-center text-[12px] text-[color:var(--ss-text-dim)]">
                {family.length < 2
                  ? "Run a parameter sweep to see metric-vs-value here."
                  : "This metric has no values across the family."}
              </div>
            )}
          </div>
        ) : view === "xy" ? (
          <div className="min-h-0 flex-[3] p-1" ref={chartHost}>
            {xyData.length > 0 && hasSize ? (
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={xyData} margin={{ top: 8, right: 16, bottom: 18, left: 4 }}>
                  <CartesianGrid stroke={theme === "dark" ? "#2a2f37" : "#eceff3"} />
                  <XAxis
                    dataKey="x"
                    type="number"
                    domain={["dataMin", "dataMax"]}
                    tick={{ fontSize: 10 }}
                    label={{
                      value: `${xyXShort}${xyXChannel?.unit ? ` [${xyXChannel.unit}]` : ""}`,
                      position: "insideBottom",
                      offset: -8,
                      fontSize: 11,
                    }}
                  />
                  {xyYUnits.map((u, i) => (
                    <YAxis
                      key={u}
                      yAxisId={u}
                      orientation={i % 2 === 0 ? "left" : "right"}
                      tick={{ fontSize: 10 }}
                      width={46}
                      label={{
                        value: u,
                        angle: -90,
                        position: i % 2 === 0 ? "insideLeft" : "insideRight",
                        fontSize: 10,
                        style: { textAnchor: "middle" },
                      }}
                    />
                  ))}
                  <Tooltip
                    cursor={{ stroke: "var(--ss-accent)", strokeWidth: 1, strokeDasharray: "3 3" }}
                    contentStyle={{
                      fontSize: 11,
                      background: "var(--ss-panel)",
                      border: "1px solid var(--ss-border)",
                      color: "var(--ss-text)",
                    }}
                    labelFormatter={(x) =>
                      `${xyXShort} = ${typeof x === "number" ? x.toLocaleString(undefined, { maximumFractionDigits: 3 }) : x}${xyXChannel?.unit ? ` ${xyXChannel.unit}` : ""}`
                    }
                    formatter={(value, name) => {
                      const c = channelByKey.get(String(name));
                      const v =
                        typeof value === "number"
                          ? value.toLocaleString(undefined, { maximumFractionDigits: 3 })
                          : String(value ?? "");
                      return [`${v} ${c?.unit ?? ""}`, c ? (c.label.split(" · ")[1] ?? c.label) : String(name)];
                    }}
                  />
                  <Legend
                    formatter={(key: string) => {
                      const c = channelByKey.get(key);
                      return <span style={{ fontSize: 10 }}>{c ? (c.label.split(" · ")[1] ?? c.label) : key}</span>;
                    }}
                  />
                  {xyYChannels.map((c) => {
                    const key = channelKey(c);
                    return (
                      <Line
                        key={key}
                        yAxisId={c.unit}
                        dataKey={key}
                        type="linear"
                        dot={false}
                        strokeWidth={1.6}
                        stroke={channelColor(key)}
                        connectNulls
                        isAnimationActive={false}
                      />
                    );
                  })}
                </LineChart>
              </ResponsiveContainer>
            ) : (
              <div className="flex h-full items-center justify-center px-4 text-center text-[12px] text-[color:var(--ss-text-dim)]">
                {selectedKeys.size < 2
                  ? "Tick at least two channels on the left — one becomes the X axis, the rest are plotted against it."
                  : "No samples to plot for this pair."}
              </div>
            )}
          </div>
        ) : (
          <div className="min-h-0 flex-[3] p-1" ref={chartHost}>
            {chartData.length > 0 && hasSize ? (
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={chartData} margin={{ top: 8, right: 12, bottom: 4, left: 4 }}>
                  <CartesianGrid stroke={theme === "dark" ? "#2a2f37" : "#eceff3"} />
                  <XAxis
                    dataKey="t"
                    type="number"
                    domain={["dataMin", "dataMax"]}
                    tick={{ fontSize: 10 }}
                    label={{ value: "t [s]", position: "insideBottomRight", fontSize: 10, offset: -2 }}
                  />
                  {units.map((u, i) => (
                    <YAxis
                      key={u}
                      yAxisId={u}
                      orientation={i % 2 === 0 ? "left" : "right"}
                      tick={{ fontSize: 10 }}
                      width={46}
                      label={{
                        value: u,
                        angle: -90,
                        position: i % 2 === 0 ? "insideLeft" : "insideRight",
                        fontSize: 10,
                        style: { textAnchor: "middle" },
                      }}
                    />
                  ))}
                  <Tooltip
                    cursor={{ stroke: "var(--ss-accent)", strokeWidth: 1, strokeDasharray: "3 3" }}
                    contentStyle={{
                      fontSize: 11,
                      background: "var(--ss-panel)",
                      border: "1px solid var(--ss-border)",
                      color: "var(--ss-text)",
                    }}
                    labelFormatter={(t) => `t = ${t} s`}
                    formatter={(value, name) => {
                      const def = defByKey.get(String(name));
                      const v =
                        typeof value === "number"
                          ? value.toLocaleString(undefined, { maximumFractionDigits: 3 })
                          : String(value ?? "");
                      return [`${v} ${def?.unit ?? ""}`, def?.legend ?? String(name)];
                    }}
                  />
                  <Legend
                    formatter={(key: string) => (
                      <span style={{ fontSize: 10 }}>{defByKey.get(key)?.legend ?? key}</span>
                    )}
                  />
                  {seriesDefs.map((d) => (
                    <Line
                      key={d.dataKey}
                      yAxisId={d.unit}
                      dataKey={d.dataKey}
                      type="linear"
                      dot={false}
                      strokeWidth={1.6}
                      strokeDasharray={d.dash || undefined}
                      stroke={d.color}
                      connectNulls
                      isAnimationActive={false}
                    />
                  ))}
                </LineChart>
              </ResponsiveContainer>
            ) : (
              <div className="flex h-full items-center justify-center text-[12px] text-[color:var(--ss-text-dim)]">
                Tick channels on the left to plot them.
              </div>
            )}
          </div>
        )}

        {result && result.summary.length > 0 && view !== "sweep" && (
          <div className="max-h-[130px] shrink-0 overflow-auto border-t border-[color:var(--ss-border)]">
            <table className="w-full border-collapse">
              <thead className="sticky top-0">
                <tr>
                  <th className="ss-th">Summary value</th>
                  {plotRuns.map((r, i) => (
                    <th key={r.id} className="ss-th w-[110px] text-right" title={runLabel(r)}>
                      <span style={{ color: multiRun ? runColor(i) : undefined }}>
                        {multiRun ? runShort(r) : "Value"}
                      </span>
                    </th>
                  ))}
                  <th className="ss-th w-[56px]">Unit</th>
                </tr>
              </thead>
              <tbody>
                {result.summary.map((s, i) => (
                  <tr key={i} className="hover:bg-[color:var(--ss-hover)]">
                    <td className="ss-td">{s.label}</td>
                    {plotRuns.map((r, ri) => {
                      const sv = r.result.summary.find((x) => x.label === s.label);
                      const v = sv?.value;
                      return (
                        <td
                          key={r.id}
                          className={`ss-td text-right font-mono ${ri > 0 ? "text-[color:var(--ss-text-dim)]" : ""}`}
                        >
                          {typeof v === "number" ? v.toLocaleString() : "—"}
                          {sv?.notValid && (
                            <div
                              className="font-sans text-[10px] text-amber-600"
                              title={`Not valid: ${sv.notValid}`}
                            >
                              not valid: {sv.notValid}
                            </div>
                          )}
                        </td>
                      );
                    })}
                    <td className="ss-td text-[color:var(--ss-text-dim)]">{s.unit}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
