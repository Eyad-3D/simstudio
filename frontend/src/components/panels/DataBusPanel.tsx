import { useMemo, useState } from "react";
import { Cable, Trash2 } from "lucide-react";
import { portsOf, useProjectStore } from "../../store/projectStore";
import { componentIcon } from "../../icons";
import type { ElementInstance, PortDef } from "../../types";

interface Sel {
  elementId: string | null;
  portId: string | null;
}

function ElementColumn({
  title,
  sel,
  onSelect,
}: {
  title: string;
  sel: Sel;
  onSelect: (elementId: string) => void;
}) {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const [filter, setFilter] = useState("");

  const elements = useMemo(() => {
    if (!project) return [];
    const q = filter.trim().toLowerCase();
    return project.systems
      .flatMap((s) => s.elements)
      .filter((el) => {
        // a Monitor or Script is listed before it has ports of its own
        const def = libraryById[el.componentDefId];
        if (!def?.allowDynamicPorts && !def?.ports.some((p) => p.kind === "signal")) return false;
        return !q || el.label.toLowerCase().includes(q);
      });
  }, [project, libraryById, filter]);

  return (
    <div className="flex min-w-0 flex-1 flex-col border-r border-[color:var(--ss-border)]">
      <div className="ss-panel-toolbar">
        <span className="text-[11px] font-semibold">{title}</span>
        <input
          className="ss-input ml-auto w-[110px]"
          placeholder="Filter…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto py-0.5">
        {elements.map((el) => {
          const def = libraryById[el.componentDefId];
          const Icon = componentIcon(def?.icon ?? "box");
          return (
            <button
              key={el.id}
              className={`ss-tree-row ${sel.elementId === el.id ? "selected" : ""}`}
              onClick={() => onSelect(el.id)}
            >
              <Icon size={13} strokeWidth={1.6} className="shrink-0 text-[color:var(--ss-node-icon)]" />
              <span className="truncate">{el.label}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function PortColumn({
  element,
  sel,
  onSelect,
}: {
  element: ElementInstance | undefined;
  sel: Sel;
  onSelect: (portId: string) => void;
}) {
  const libraryById = useProjectStore((s) => s.libraryById);
  const ports: PortDef[] = element
    ? portsOf(element, libraryById).filter((p) => p.kind === "signal")
    : [];
  return (
    <div className="flex min-w-0 flex-1 flex-col border-r border-[color:var(--ss-border)]">
      <table className="w-full border-collapse">
        <thead>
          <tr>
            <th className="ss-th">Port</th>
            <th className="ss-th w-[76px]">Unit Group</th>
            <th className="ss-th w-[46px]">Dir</th>
          </tr>
        </thead>
        <tbody>
          {ports.map((p) => (
            <tr
              key={p.id}
              className={`cursor-pointer hover:bg-[color:var(--ss-hover)] ${
                sel.portId === p.id ? "bg-[color:var(--ss-accent-soft)]" : ""
              }`}
              onClick={() => onSelect(p.id)}
            >
              <td className="ss-td truncate">{p.name}</td>
              <td className="ss-td text-[11px] text-[color:var(--ss-text-dim)]">
                {p.unitGroup ?? "No Unit"}
              </td>
              <td className="ss-td text-[10px] uppercase text-[color:var(--ss-text-dim)]">
                {p.direction === "output" ? "out" : p.direction === "input" ? "in" : "i/o"}
              </td>
            </tr>
          ))}
          {ports.length === 0 && (
            <tr>
              <td className="ss-td text-[11px] text-[color:var(--ss-text-dim)]" colSpan={3}>
                {element ? "No ports yet: add one in Properties." : "Select an element."}
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

export function DataBusPanel() {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const addDataBus = useProjectStore((s) => s.addDataBus);
  const removeDataBus = useProjectStore((s) => s.removeDataBus);

  const [sel1, setSel1] = useState<Sel>({ elementId: null, portId: null });
  const [sel2, setSel2] = useState<Sel>({ elementId: null, portId: null });

  const allElements = useMemo(
    () => new Map((project?.systems.flatMap((s) => s.elements) ?? []).map((e) => [e.id, e])),
    [project],
  );

  const portName = (elId: string, portId: string) => {
    const el = allElements.get(elId);
    return (el && portsOf(el, libraryById).find((p) => p.id === portId)?.name) ?? portId;
  };

  const canConnect = sel1.elementId && sel1.portId && sel2.elementId && sel2.portId;

  return (
    <div className="flex h-full">
      <ElementColumn
        title="Element 1"
        sel={sel1}
        onSelect={(elementId) => setSel1({ elementId, portId: null })}
      />
      <PortColumn
        element={sel1.elementId ? allElements.get(sel1.elementId) : undefined}
        sel={sel1}
        onSelect={(portId) => setSel1((s) => ({ ...s, portId }))}
      />
      <ElementColumn
        title="Element 2"
        sel={sel2}
        onSelect={(elementId) => setSel2({ elementId, portId: null })}
      />
      <PortColumn
        element={sel2.elementId ? allElements.get(sel2.elementId) : undefined}
        sel={sel2}
        onSelect={(portId) => setSel2((s) => ({ ...s, portId }))}
      />
      <div className="flex w-[340px] shrink-0 flex-col">
        <div className="ss-panel-toolbar">
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] disabled:opacity-40"
            disabled={!canConnect}
            onClick={() => {
              if (canConnect) {
                addDataBus(sel1.elementId!, sel1.portId!, sel2.elementId!, sel2.portId!);
              }
            }}
          >
            <Cable size={13} /> Connect
          </button>
          <span className="ml-auto text-[11px] text-[color:var(--ss-text-dim)]">
            {project?.dataBusConnections.length ?? 0} connection(s)
          </span>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {(project?.dataBusConnections ?? []).map((d) => (
            <div
              key={d.id}
              className="flex items-center gap-1 border-b border-[color:var(--ss-td-border)] px-2 py-1 text-[11px] hover:bg-[color:var(--ss-hover)]"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate">
                  <b>{allElements.get(d.element1Id)?.label ?? "?"}</b>
                  <span className="text-[color:var(--ss-text-dim)]">
                    {" "}
                    · {portName(d.element1Id, d.port1Id)}
                  </span>
                </div>
                <div className="truncate">
                  ↔ <b>{allElements.get(d.element2Id)?.label ?? "?"}</b>
                  <span className="text-[color:var(--ss-text-dim)]">
                    {" "}
                    · {portName(d.element2Id, d.port2Id)}
                  </span>
                </div>
              </div>
              <button
                className="ss-toolbtn shrink-0"
                title="Remove connection"
                onClick={() => removeDataBus(d.id)}
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))}
          {(project?.dataBusConnections.length ?? 0) === 0 && (
            <div className="px-3 py-2 text-[11px] text-[color:var(--ss-text-dim)]">
              Pick a signal port on each side, then click Connect.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
