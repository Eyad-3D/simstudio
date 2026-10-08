// The Results page's Energy view (RES-22): a Sankey chart of where the
// sources' energy went, and the energy in, out and lost of every part.
import { useMemo, useRef } from "react";
import { Download, ImageIcon } from "lucide-react";
import { downloadText, energyCsv, sankeyLayout, type SankeyNode } from "../../reports";
import { useUIStore } from "../../store/uiStore";
import type { SimRun } from "../../types";

const W = 1040; // the chart's drawing width; it scales to the panel
const NODE_W = 12;
// sources (labels to their left), the energy in, the groups (labels to
// their left, over the bands), each sink (labels to its right)
const COL_X = [210, 310, 510, 700];
const LABEL_GAP = 13; // px between label baselines
const kwh = (v: number) => v.toLocaleString(undefined, { maximumFractionDigits: v >= 10 ? 2 : 3, minimumFractionDigits: 0 });
const pct = (v: number, total: number) => (total > 0 ? `${((100 * v) / total).toFixed(1)} %` : "");
// what "not accounted for" is, in the header and the table's last row
const REMAINDER_HINT =
  "The sources' energy less every place it went. Every part's own books close, so this is only where the energy out of one part is not quite the energy into the next: the solver's step, which grows with hard wheel spin and with a coarse step. It is the summary's Energy balance residual, there as a share of the energy the sources gave up and with the sign the other way round (energy the step made is + there, − here). A large one means the model does not add up.";

/** Label positions for one column, pushed apart so they never overlap. */
function spread(nodes: SankeyNode[]): Map<string, number> {
  const out = new Map<string, number>();
  let last = -Infinity;
  for (const n of [...nodes].sort((a, b) => a.y - b.y)) {
    const y = Math.max(n.y + n.h / 2, last + LABEL_GAP);
    out.set(n.id, y);
    last = y;
  }
  return out;
}

/** Select and frame a part on the diagram, as the Problems list does. */
function showPart(elementId: string) {
  const ui = useUIStore.getState();
  ui.setRibbonTab("home");
  ui.focusPanel("topology");
  ui.revealElements?.([elementId]);
}

export function EnergyView({ run }: { run: SimRun }) {
  const theme = useUIStore((s) => s.theme);
  const e = run.result.energy;
  const svgRef = useRef<SVGSVGElement>(null);
  const layout = useMemo(() => (e ? sankeyLayout(e, theme === "dark") : null), [e, theme]);

  if (!e || !layout) {
    const off = run.snapshot?.case.energyReport === false;
    return (
      <div className="flex h-full min-h-[200px] items-center justify-center px-4 text-center text-[12px] text-[color:var(--ss-text-dim)]">
        {run.status === "running"
          ? "The energy report comes when the run finishes."
          : off
            ? "This case's Energy report is off: tick Energy report in the Cases tab and run it again."
            : "This run has no energy report: it failed, or it was made before LightSim 0.3. Run the case again."}
      </div>
    );
  }

  const total = e.sourceKWh;
  const { nodes, links } = layout;
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const top = 16;
  const labels = [0, 2, 3].map((c) => spread(nodes.filter((n) => n.column === c)));
  const labelY = (n: SankeyNode) => labels[[0, -1, 1, 2][n.column]]?.get(n.id) ?? n.y + n.h / 2;
  const height = Math.ceil(Math.max(layout.bottom, ...nodes.map((n) => labelY(n) + 6)) + top + 12);
  const remainderBig = Math.abs(e.remainderPct) >= 1;
  const parts = [...e.parts].sort((a, b) => b.lostKWh - a.lostKWh);

  const exportSvg = () => {
    const svg = svgRef.current;
    if (!svg) return;
    // the text colours come from the theme's CSS variables: write them in
    const copy = svg.cloneNode(true) as SVGSVGElement;
    const css = getComputedStyle(document.documentElement);
    const ink = css.getPropertyValue("--ss-text").trim() || "#222";
    const dim = css.getPropertyValue("--ss-text-dim").trim() || "#666";
    const bg = css.getPropertyValue("--ss-panel").trim() || "#fff";
    copy.querySelectorAll("[data-ink]").forEach((t) => t.setAttribute("fill", t.getAttribute("data-ink") === "dim" ? dim : ink));
    copy.querySelectorAll("[data-halo]").forEach((t) => t.setAttribute("stroke", bg));
    copy.setAttribute("xmlns", "http://www.w3.org/2000/svg");
    copy.insertAdjacentHTML("afterbegin", `<rect width="100%" height="100%" fill="${bg}"/>`);
    downloadText(new XMLSerializer().serializeToString(copy), `lightsim-${run.caseName}-energy.svg`, "image/svg+xml");
  };

  return (
    <div className="flex min-h-[260px] flex-[3] flex-col overflow-auto">
      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 px-2 py-1 text-[11px] text-[color:var(--ss-text-dim)]">
        <span>
          Sources <b className="font-mono text-[color:var(--ss-text)]">{kwh(total)} kWh</b>
        </span>
        <span className={remainderBig ? "text-[color:var(--ss-warn)]" : ""} title={REMAINDER_HINT}>
          Not accounted for (energy balance residual){" "}
          <b className="font-mono">
            {kwh(e.remainderKWh)} kWh ({e.remainderPct.toFixed(2)} %)
          </b>
        </span>
        {e.balanceErrorPct != null && (
          <span title="The summary's Electrical energy balance error: energy no electrical source supplied or took. Above 0.1 % the run's energy figures are marked not valid.">
            Electrical energy balance error <b className="font-mono">{e.balanceErrorPct} %</b>
          </span>
        )}
        <div className="ml-auto flex items-center gap-1">
          <button className="ss-toolbtn border border-[color:var(--ss-border)]" title="Save the Sankey chart as an SVG image" onClick={exportSvg}>
            <ImageIcon size={12} /> SVG
          </button>
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)]"
            title="Save the energy table and the chart's bands as CSV"
            onClick={() => downloadText(energyCsv(e), `lightsim-${run.caseName}-energy.csv`)}
          >
            <Download size={12} /> CSV
          </button>
        </div>
      </div>

      <svg
        ref={svgRef}
        role="img"
        aria-label={`Energy Sankey chart: ${e.sources.map((f) => `${f.label} ${kwh(f.kWh)} kWh`).join(", ")}, to ${e.sinks.length} places`}
        viewBox={`0 0 ${W} ${height}`}
        className="mx-auto block w-full max-w-[1100px] shrink-0"
        style={{ minHeight: 220 }}
        fontSize={11}
      >
        <g transform={`translate(0 ${top})`}>
          {links.map((l, i) => {
            const a = byId.get(l.from)!;
            const b = byId.get(l.to)!;
            const x0 = COL_X[a.column] + NODE_W;
            const x1 = COL_X[b.column];
            const xm = (x0 + x1) / 2;
            const h = Math.max(l.h, 0.5);
            const d = `M${x0},${l.y0} C${xm},${l.y0} ${xm},${l.y1} ${x1},${l.y1} L${x1},${l.y1 + h} C${xm},${l.y1 + h} ${xm},${l.y0 + h} ${x0},${l.y0 + h} Z`;
            return (
              <path key={i} d={d} fill={l.color} fillOpacity={0.4}>
                <title>
                  {a.label} → {b.label}: {kwh(l.kWh)} kWh ({pct(l.kWh, total)} of the sources)
                </title>
              </path>
            );
          })}
          {nodes.map((n) => {
            const x = COL_X[n.column];
            const clickable = Boolean(n.elementId);
            const ly = labelY(n);
            return (
              <g
                key={n.id}
                className={clickable ? "cursor-pointer" : undefined}
                onClick={clickable ? () => showPart(n.elementId!) : undefined}
              >
                <rect x={x} y={n.y} width={NODE_W} height={Math.max(n.h, 1)} rx={2} fill={n.color}>
                  <title>
                    {n.label}: {kwh(n.kWh)} kWh ({pct(n.kWh, total)} of the sources){clickable ? " — click to show it on the diagram" : ""}
                  </title>
                </rect>
                {n.column !== 1 && (
                  <text
                    x={n.column === 3 ? x + NODE_W + 6 : x - 6}
                    y={ly}
                    dominantBaseline="middle"
                    textAnchor={n.column === 3 ? "start" : "end"}
                    data-ink="ink"
                    data-halo=""
                    fill="var(--ss-text)"
                    stroke="var(--ss-panel)"
                    strokeWidth={3}
                    paintOrder="stroke"
                    fontWeight={n.column === 2 ? 600 : 400}
                  >
                    {n.label}
                    {n.column !== 2 && (
                      <tspan data-ink="dim" fill="var(--ss-text-dim)">
                        {" "}
                        {kwh(n.kWh)} kWh · {pct(n.kWh, total)}
                      </tspan>
                    )}
                  </text>
                )}
                {n.column === 1 && (
                  <text x={x + NODE_W / 2} y={-5} textAnchor="middle" data-ink="dim" fill="var(--ss-text-dim)">
                    {n.label} {kwh(n.kWh)} kWh
                  </text>
                )}
              </g>
            );
          })}
        </g>
      </svg>

      <div className="shrink-0 px-2 pb-2">
        <table className="w-full border-collapse" aria-label="Energy per part">
          <thead>
            <tr>
              <th className="ss-th">Part</th>
              <th className="ss-th text-right" title="Energy that went into the part">In [kWh]</th>
              <th className="ss-th text-right" title="Energy that came out of it">Out [kWh]</th>
              <th className="ss-th text-right" title="Energy it lost as heat, or a consumer used">Lost [kWh]</th>
              <th className="ss-th text-right" title="The change of the energy it stores (a battery's charge, the car's speed and height): negative when it gave some up">
                Stored change [kWh]
              </th>
              <th className="ss-th text-right" title="Its loss as a share of the sources' energy">Lost [% of sources]</th>
            </tr>
          </thead>
          <tbody>
            {parts.map((p, i) => (
              <tr key={i} className="hover:bg-[color:var(--ss-hover)]">
                <td className="ss-td">
                  {p.elementId ? (
                    <button
                      className="text-left text-[color:var(--ss-accent)] hover:underline"
                      title="Show this part on the diagram"
                      onClick={() => showPart(p.elementId!)}
                    >
                      {p.label}
                    </button>
                  ) : (
                    <span title="A driveline's spinning parts (motor rotors, gears, shafts and wheels) together: the energy in their speed">{p.label}</span>
                  )}
                </td>
                <td className="ss-td text-right font-mono">{kwh(p.inKWh)}</td>
                <td className="ss-td text-right font-mono">{kwh(p.outKWh)}</td>
                <td className="ss-td text-right font-mono">{kwh(p.lostKWh)}</td>
                <td className="ss-td text-right font-mono">{p.storedKWh ? kwh(p.storedKWh) : "—"}</td>
                <td className="ss-td text-right font-mono">{p.lostPct.toFixed(1)}</td>
              </tr>
            ))}
            <tr className="text-[color:var(--ss-text-dim)]">
              <td className="ss-td" title={REMAINDER_HINT}>
                Not accounted for (energy balance residual)
              </td>
              <td className="ss-td" />
              <td className="ss-td" />
              <td className="ss-td text-right font-mono">{kwh(e.remainderKWh)}</td>
              <td className="ss-td" />
              <td className="ss-td text-right font-mono">{e.remainderPct.toFixed(1)}</td>
            </tr>
          </tbody>
        </table>
        <p className="mt-1 text-[10px] text-[color:var(--ss-text-dim)]">
          Each part books itself every solver step, and its In − Out − Lost − Stored change is 0. In and Out count both ways: a
          wheel's In is what its axle gave it when driving and what the car gave it when braking, and its Lost is the tyre's slip.
          In a lap case the gears and friction brakes are in the Vehicle's row, from the lap's own books.
        </p>
      </div>
    </div>
  );
}
