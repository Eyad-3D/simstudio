import { useMemo } from "react";
import { Gauge } from "lucide-react";
import { portsOf, useActiveRun, useProjectStore } from "../../store/projectStore";
import type { Channel } from "../../types";

/** Tiny inline sparkline over a channel's recent history. */
function Sparkline({ series }: { series: { t: number; value: number | null }[] }) {
  const w = 120;
  const h = 28;
  const tail = series.slice(-240).filter((p): p is { t: number; value: number } => p.value !== null);
  if (tail.length < 2) {
    return <div style={{ width: w, height: h }} className="rounded bg-[color:var(--ss-panel-alt)]" />;
  }
  let min = Infinity;
  let max = -Infinity;
  for (const p of tail) {
    if (p.value < min) min = p.value;
    if (p.value > max) max = p.value;
  }
  const span = max - min || 1;
  const pts = tail
    .map((p, i) => {
      const x = (i / (tail.length - 1)) * w;
      const y = h - 2 - ((p.value - min) / span) * (h - 4);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg width={w} height={h} className="rounded bg-[color:var(--ss-panel-alt)]">
      <polyline points={pts} fill="none" stroke="var(--ss-accent)" strokeWidth="1.4" />
    </svg>
  );
}

interface MonitorInput {
  portName: string;
  sourceKey: string; // "elementId:portId"
  sourceLabel: string;
  unit: string;
}

export function MonitorsPanel() {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const unitGroups = useProjectStore((s) => s.unitGroups);
  const liveValues = useProjectStore((s) => s.liveValues);
  const running = useProjectStore((s) => s.running);
  const select = useProjectStore((s) => s.select);

  const result = useActiveRun()?.result;

  const monitors = useMemo(() => {
    if (!project) return [];
    const elements = project.systems.flatMap((s) => s.elements);
    const byId = new Map(elements.map((e) => [e.id, e]));
    return elements
      .filter((e) => libraryById[e.componentDefId]?.id === "signal.monitor")
      .map((mon) => {
        const inputs: MonitorInput[] = [];
        for (const port of mon.dynamicPorts ?? []) {
          if (port.direction !== "input") continue;
          // find the data-bus wire feeding this input
          const dbc = project.dataBusConnections.find(
            (d) =>
              (d.element1Id === mon.id && d.port1Id === port.id) ||
              (d.element2Id === mon.id && d.port2Id === port.id),
          );
          if (!dbc) {
            inputs.push({ portName: port.name, sourceKey: "", sourceLabel: "not wired", unit: "" });
            continue;
          }
          const [srcEl, srcPort] =
            dbc.element1Id === mon.id ? [dbc.element2Id, dbc.port2Id] : [dbc.element1Id, dbc.port1Id];
          const src = byId.get(srcEl);
          const pdef = src && portsOf(src, libraryById).find((p) => p.id === srcPort);
          const unit = unitGroups[pdef?.unitGroup ?? "No Unit"] ?? "-";
          inputs.push({
            portName: port.name,
            sourceKey: `${srcEl}:${srcPort}`,
            sourceLabel: src && pdef ? `${src.label} · ${pdef.name}` : "unknown",
            unit,
          });
        }
        return { element: mon, inputs };
      });
  }, [project, libraryById, unitGroups]);

  const channelByKey = useMemo(() => {
    const map = new Map<string, Channel>();
    for (const c of result?.channels ?? []) {
      map.set(`${c.elementId}:${c.portId}`, c);
    }
    return map;
  }, [result]);

  if (monitors.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-4 text-center text-[12px] text-[color:var(--ss-text-dim)]">
        <Gauge size={30} strokeWidth={1} />
        <div>
          No Monitor elements in the model. Drop a <b>Monitor</b> from the library, add
          input ports in its Properties, and wire signals into them — live readouts
          appear here during a run.
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-wrap content-start gap-2 overflow-y-auto p-2">
      {monitors.map(({ element, inputs }) => (
        <div
          key={element.id}
          className="min-w-[230px] rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-sm"
        >
          <button
            className="flex w-full items-center gap-1.5 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-2 py-1 text-[11px] font-semibold hover:text-[color:var(--ss-accent)]"
            onClick={() => select(element.id)}
            title="Select this monitor"
          >
            <Gauge size={12} />
            {element.label}
            {running && (
              <span className="ml-auto flex items-center gap-1 text-[9px] font-semibold uppercase text-emerald-700">
                <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500" />
                live
              </span>
            )}
          </button>
          <div className="flex flex-col gap-1 p-2">
            {inputs.length === 0 && (
              <div className="text-[11px] italic text-[color:var(--ss-text-dim)]">
                No input ports — add them in Properties.
              </div>
            )}
            {inputs.map((inp, i) => {
              const value = inp.sourceKey ? liveValues[inp.sourceKey] : undefined;
              const channel = inp.sourceKey ? channelByKey.get(inp.sourceKey) : undefined;
              const shown =
                value ?? channel?.timeSeries[channel.timeSeries.length - 1]?.value;
              return (
                <div key={i} className="flex items-center gap-2">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[10px] text-[color:var(--ss-text-dim)]" title={inp.sourceLabel}>
                      {inp.portName} — {inp.sourceLabel}
                    </div>
                    <div className="font-mono text-[15px] leading-tight">
                      {shown == null
                        ? "—"
                        : shown.toLocaleString(undefined, { maximumFractionDigits: 2 })}
                      <span className="ml-1 text-[10px] text-[color:var(--ss-text-dim)]">
                        {inp.unit}
                      </span>
                    </div>
                  </div>
                  <Sparkline series={channel?.timeSeries ?? []} />
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
