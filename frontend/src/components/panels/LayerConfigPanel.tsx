import { Activity } from "lucide-react";
import { useUIStore, type EdgeKindFilter } from "../../store/uiStore";
import { KIND_COLOR } from "../canvas/ElementNode";

const LAYERS: { id: EdgeKindFilter; label: string; color: string; note: string }[] = [
  {
    id: "electrical",
    label: "Electrical connections",
    color: KIND_COLOR.electrical,
    note: "Battery, DC-DC, e-motor supply lines (also covers thermal/fluid)",
  },
  {
    id: "mechanical",
    label: "Mechanical connections",
    color: KIND_COLOR.mechanical,
    note: "Shafts, gearbox, differential and wheel couplings",
  },
  {
    id: "signal",
    label: "Signal / data-bus links",
    color: KIND_COLOR.signal,
    note: "Control & sensor wiring (dashed) — reveals your control loops on the canvas",
  },
];

export function LayerConfigPanel() {
  const visibleKinds = useUIStore((s) => s.visibleKinds);
  const toggleKind = useUIStore((s) => s.toggleKind);
  const showLiveValues = useUIStore((s) => s.showLiveValues);
  const toggleLiveValues = useUIStore((s) => s.toggleLiveValues);

  return (
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar text-[11px] text-[color:var(--ss-text-dim)]">
        Choose which connection layers and overlays are drawn on the Topology canvas.
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        <table className="w-full border-collapse">
          <thead>
            <tr>
              <th className="ss-th w-[52px]">Visible</th>
              <th className="ss-th w-[220px]">Layer</th>
              <th className="ss-th">Contents</th>
            </tr>
          </thead>
          <tbody>
            {LAYERS.map((l) => (
              <tr key={l.id} className="hover:bg-[color:var(--ss-hover)]">
                <td className="ss-td text-center">
                  <input
                    type="checkbox"
                    checked={visibleKinds[l.id]}
                    onChange={() => toggleKind(l.id)}
                  />
                </td>
                <td className="ss-td">
                  <span className="flex items-center gap-2">
                    <span
                      className="inline-block h-[3px] w-6"
                      style={{
                        background: l.color,
                        ...(l.id === "signal"
                          ? { backgroundImage: "none", borderTop: `2px dashed ${l.color}`, height: 0 }
                          : {}),
                      }}
                    />
                    {l.label}
                  </span>
                </td>
                <td className="ss-td text-[11px] text-[color:var(--ss-text-dim)]">{l.note}</td>
              </tr>
            ))}
          </tbody>
        </table>

        <div className="mt-3 mb-1 flex items-center gap-1 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
          <Activity size={12} /> Overlays
        </div>
        <label className="flex cursor-pointer items-start gap-2 rounded px-1 py-1 hover:bg-[color:var(--ss-hover)]">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={showLiveValues}
            onChange={toggleLiveValues}
          />
          <span className="text-[12px]">
            Live values on nodes
            <span className="block text-[11px] text-[color:var(--ss-text-dim)]">
              Show each element's headline signal (speed, SOC, torque…) as a chip during and
              after a run.
            </span>
          </span>
        </label>
      </div>
    </div>
  );
}
