import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  Handle,
  NodeResizer,
  Position,
  useUpdateNodeInternals,
  type NodeProps,
  type Node,
} from "@xyflow/react";
import { AlertTriangle } from "lucide-react";
import type { ComponentDef, ElementInstance, PortDef, PortKind, PortSide } from "../../types";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { componentIcon } from "../../icons";
import { NodeMarks } from "./EnergyOverlay";
import { SIDES, movedPlace, nodeSize, portPlaces, physicalPorts, type Place } from "./portPlaces";

export type ElementNodeData = {
  element: ElementInstance;
  def: ComponentDef;
};

export type ElementFlowNode = Node<ElementNodeData, "element">;

export { DEFAULT_NODE_WIDTH } from "./portPlaces";

// One colour per kind of connection for wires, node borders and icons, pins,
// layer swatches and port dots: the theme's --ss-kind-* tokens (index.css),
// each at least 3:1 on the diagram in both themes and told apart with the
// common colour-vision deficiencies (contrast.test.ts). CSS values, for styles only.
export const KIND_COLOR: Record<PortKind, string> = {
  electrical: "var(--ss-kind-electrical)",
  mechanical: "var(--ss-kind-mechanical)",
  signal: "var(--ss-kind-signal)",
  thermal: "var(--ss-kind-thermal)",
  fluid: "var(--ss-kind-fluid)",
  power: "var(--ss-kind-electrical)",
};

/** A wire's line by its kind, so that colour is never the only cue (UI
 *  4.42): electrical thin and solid, mechanical (a shaft) thicker and solid,
 *  thermal dotted, fluid dashed; signal links are dashed too, and drawn only
 *  in the Data Bus overlay. SVG presentation values. */
export const KIND_LINE: Record<PortKind, { strokeWidth: number; strokeDasharray?: string; strokeLinecap?: "round" }> = {
  electrical: { strokeWidth: 1.6 },
  power: { strokeWidth: 1.6 },
  mechanical: { strokeWidth: 2.6 },
  thermal: { strokeWidth: 2, strokeDasharray: "1 4", strokeLinecap: "round" },
  fluid: { strokeWidth: 2, strokeDasharray: "7 3" },
  signal: { strokeWidth: 1.4, strokeDasharray: "5 4" },
};

// The one "headline" signal shown as a live chip on a node during/after a run.
// Keyed by componentDefId → {port, unit, digits}. Elements not listed show none.
const LIVE_BADGE: Record<string, { port: string; unit: string; digits: number }> = {
  "vehicle.body": { port: "sig_speed", unit: "km/h", digits: 1 },
  "battery.generic": { port: "sig_soc", unit: "%", digits: 1 },
  "motor.emotor": { port: "sig_torque", unit: "N·m", digits: 0 },
  "engine.combustion": { port: "sig_speed", unit: "1/min", digits: 0 },
  "fuelcell.stack": { port: "sig_power", unit: "kW", digits: 1 },
  "electric.voltage_source": { port: "sig_power", unit: "kW", digits: 1 },
  "electric.constant_drive": { port: "sig_power", unit: "kW", digits: 1 },
  "controller.dcdc": { port: "sig_power_out", unit: "kW", digits: 1 },
  "mech.final_drive": { port: "sig_speed_out", unit: "1/min", digits: 0 },
  "propulsion.wheel": { port: "sig_speed", unit: "1/min", digits: 0 },
  "fuel.tank": { port: "sig_level", unit: "%", digits: 0 },
  "fuel.h2_tank": { port: "sig_level", unit: "%", digits: 0 },
  "signal.driving_task": { port: "sig_demand", unit: "km/h", digits: 0 },
  "driver.driver": { port: "sig_accel_pedal", unit: "", digits: 2 },
};

function formatLive(v: number, digits: number): string {
  if (!Number.isFinite(v)) return "—";
  return Math.abs(v) >= 1000 ? Math.round(v).toLocaleString() : v.toFixed(digits);
}

const POSITION_OF: Record<PortSide, Position> = {
  left: Position.Left,
  right: Position.Right,
  top: Position.Top,
  bottom: Position.Bottom,
};

/** Nearest node edge + offset (0..1 along that edge) for a screen point. */
function placementFromPoint(rect: DOMRect, clientX: number, clientY: number): Place {
  const x = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  const y = Math.min(1, Math.max(0, (clientY - rect.top) / rect.height));
  const d = { left: x, right: 1 - x, top: y, bottom: 1 - y };
  const side = (Object.keys(d) as PortSide[]).reduce((a, b) => (d[b] < d[a] ? b : a));
  return { side, frac: side === "top" || side === "bottom" ? x : y };
}

function handleClass(kind: PortDef["kind"]): string {
  return `ss-handle-${kind}`;
}

function polarityGlyphStyle(side: PortSide, offset: string): React.CSSProperties {
  switch (side) {
    case "left":
      return { left: 3, top: offset, transform: "translateY(-50%)" };
    case "right":
      return { right: 3, top: offset, transform: "translateY(-50%)" };
    case "top":
      return { top: 2, left: offset, transform: "translateX(-50%)" };
    case "bottom":
      return { bottom: 2, left: offset, transform: "translateX(-50%)" };
  }
}

/** A physical (non-signal) port: one interactive source handle that can both
 *  start and end connections, plus an invisible target handle underneath so
 *  incoming edges have an anchor of type 'target' at the same spot.
 *  Shift+drag moves the pin round the part (it snaps to free places);
 *  Shift+click (no drag) moves it to the next side. */
function PhysicalPort({
  port,
  side,
  frac,
  boxRef,
  onCycle,
  onDragPreview,
  onDragCommit,
}: {
  port: PortDef;
  side: PortSide;
  frac: number;
  boxRef: React.RefObject<HTMLDivElement | null>;
  onCycle: (port: PortDef) => void;
  onDragPreview: (portId: string, placement: Place) => void;
  onDragCommit: (port: PortDef) => void;
}) {
  const pos = POSITION_OF[side];
  const offset = `${frac * 100}%`;
  const style: React.CSSProperties =
    side === "top" || side === "bottom" ? { left: offset } : { top: offset };

  // Shift+drag repositions the pin. We intercept in the capture phase and stop
  // propagation so React Flow (which starts a connection on the handle's
  // onMouseDown) never fires; a plain drag falls through to start a connection.
  // Shift is also the canvas's box-select key: the handle's `nokey` class
  // keeps React Flow from starting a box selection (and swallowing the press)
  // when it lands on a pin.
  const startShiftDrag = (e: React.MouseEvent) => {
    if (!e.shiftKey) return;
    e.stopPropagation();
    e.preventDefault();
    const startX = e.clientX;
    const startY = e.clientY;
    let last: Place | null = null;
    const onMove = (ev: MouseEvent) => {
      if (!boxRef.current) return;
      last = placementFromPoint(boxRef.current.getBoundingClientRect(), ev.clientX, ev.clientY);
      onDragPreview(port.id, last);
    };
    const onUp = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp, true);
      // the click that ends the press would Shift-select the node: drop it
      const swallow = (click: MouseEvent) => click.stopPropagation();
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, true));
      const moved = Math.abs(ev.clientX - startX) + Math.abs(ev.clientY - startY);
      if (moved < 4 || !last) onCycle(port);
      else onDragCommit(port);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp, true);
  };

  return (
    <>
      <Handle
        id={port.id}
        type="target"
        position={pos}
        style={{ ...style, pointerEvents: "none", opacity: 0 }}
        isConnectableStart={false}
      />
      <Handle
        id={port.id}
        type="source"
        position={pos}
        style={{ ...style, touchAction: "none" }}
        className={`${handleClass(port.kind)} nokey`}
        isConnectableStart
        isConnectableEnd
        title={`${port.name}${port.unitText ? ` [${port.unitText}]` : ""} (${port.kind}) — drag: draw a wire · Shift+drag: move this port round the part · Shift+click: move it to the next side`}
        onMouseDownCapture={startShiftDrag}
      />
      {port.polarity && (
        <span
          className="pointer-events-none absolute z-10 text-meta font-bold leading-none"
          style={{
            ...polarityGlyphStyle(side, offset),
            color: port.polarity === "positive" ? "var(--ss-pole-positive)" : "var(--ss-pole-negative)",
          }}
        >
          {port.polarity === "positive" ? "+" : "−"}
        </span>
      )}
    </>
  );
}

export const ElementNode = memo(({ data, selected }: NodeProps<ElementFlowNode>) => {
  const { def, element } = data;
  const Icon = componentIcon(def.icon);
  const placePort = useProjectStore((s) => s.placePort);
  const dataChecks = useProjectStore((s) => s.dataChecks);
  const boxRef = useRef<HTMLDivElement | null>(null);

  const domainColor = KIND_COLOR[def.domain] ?? "var(--ss-node-border)";

  // live headline value: subscribe narrowly so only this node re-renders when
  // its own signal ticks. Undefined until the run publishes it; persists after.
  const badge = LIVE_BADGE[def.id];
  const badgeKey = badge ? `${element.id}:${badge.port}` : null;
  const liveVal = useProjectStore((s) => (badgeKey ? s.liveValues[badgeKey] : undefined));
  const showLiveValues = useUIStore((s) => s.showLiveValues);

  // surface data-check errors/warnings for this element right on the node
  const issue = useMemo(() => {
    const forEl =
      dataChecks?.filter((c) => c.elementId === element.id || c.elementIds?.includes(element.id)) ?? [];
    if (forEl.length === 0) return null;
    const worst = forEl.some((c) => c.level === "error") ? "error" : forEl.some((c) => c.level === "warning") ? "warning" : null;
    if (!worst) return null;
    return { level: worst, text: forEl.map((c) => c.text).join("\n") } as const;
  }, [dataChecks, element.id]);
  // the pin's snapped place while Shift+dragging a port (saved on release)
  const [preview, setPreview] = useState<{ portId: string; place: Place } | null>(null);

  // physical ports only — signal / data-bus wiring is managed in the
  // Data Bus Connections panel and is not drawn on the canvas
  const places = preview ? { ...portPlaces(element, def), [preview.portId]: preview.place } : portPlaces(element, def);
  const bySide: Record<PortSide, PortDef[]> = { left: [], right: [], top: [], bottom: [] };
  for (const p of physicalPorts(def)) bySide[places[p.id].side].push(p);
  const fracOf = (p: PortDef): number => places[p.id].frac;

  // the pin follows the pointer from one free place to the next
  // (the release is handled by the handler the press set up: it reads the
  // last place from a ref, not from that render's state)
  const lastPreview = useRef<{ portId: string; place: Place } | null>(null);
  const onDragPreview = (portId: string, raw: Place) => {
    const place = movedPlace(element, def, portId, raw.side, raw.frac);
    if (!place) return;
    lastPreview.current = { portId, place };
    setPreview(lastPreview.current);
  };
  const onDragCommit = (port: PortDef) => {
    const place = lastPreview.current?.portId === port.id ? lastPreview.current.place : null;
    lastPreview.current = null;
    setPreview(null);
    const now = places[port.id];
    if (place && (place.side !== now.side || place.frac !== now.frac)) placePort(element.id, port.id, place);
  };
  // Shift+click: the next side round with room, at about the same place
  const onCycle = (port: PortDef) => {
    const now = places[port.id];
    const at = SIDES.indexOf(now.side);
    for (let k = 1; k < 4; k++) {
      const place = movedPlace(element, def, port.id, SIDES[(at + k) % 4], now.frac);
      if (place) return placePort(element.id, port.id, place);
    }
  };

  // React Flow re-measures a node's pins only when the node's size changes
  // (TopologyCanvas keeps measured sizes on the nodes), so tell it when a pin
  // moves or flips side, or the node now shows another part's ports.
  const updateNodeInternals = useUpdateNodeInternals();
  const pinLayout = (Object.keys(bySide) as PortSide[])
    .flatMap((sd) => bySide[sd].map((p) => `${p.id}@${sd}:${fracOf(p)}`))
    .join(" ");
  const measuredPinLayout = useRef(pinLayout);
  useEffect(() => {
    if (measuredPinLayout.current === pinLayout) return;
    measuredPinLayout.current = pinLayout;
    updateNodeInternals(element.id);
  }, [pinLayout, element.id, updateNodeInternals]);

  const { width, height } = nodeSize(element, places);
  const isSub = element.isSubSystem;

  return (
    <>
      <NodeResizer
        isVisible={selected}
        minWidth={56}
        minHeight={40}
        onResizeStart={() => useProjectStore.getState().beginHistory()}
        lineClassName="ss-resize-line"
        handleClassName="ss-resize-handle"
      />
      <div
        ref={boxRef}
        className={`group relative flex items-center justify-center rounded-md border-[1.5px] bg-[color:var(--ss-panel)] shadow-sm transition-shadow
          ${isSub ? "border-dashed border-[color:var(--ss-accent)] bg-[color:var(--ss-accent-soft)]" : ""}
          ${selected ? "outline outline-2 outline-[color:var(--ss-accent)] shadow-md" : issue?.level === "error" ? "outline outline-2 outline-[color:var(--ss-err)]" : issue?.level === "warning" ? "outline outline-1 outline-[color:var(--ss-warn)]" : ""}`}
        style={{ width, height, ...(isSub ? {} : { borderColor: domainColor }) }}
        title={isSub ? "Double-click to open sub-system" : `${def.name} — double-click for parameters`}
      >
        {showLiveValues && badge && liveVal !== undefined && (
          <div
            className="pointer-events-none absolute bottom-full left-1/2 z-10 mb-1 -translate-x-1/2 whitespace-nowrap rounded px-1 py-[1px] text-meta font-semibold leading-none tabular-nums"
            style={{
              color: "var(--ss-accent)",
              background: "color-mix(in srgb, var(--ss-accent) 14%, var(--ss-panel))",
              border: "1px solid color-mix(in srgb, var(--ss-accent) 40%, transparent)",
            }}
            title={badge.port}
          >
            {formatLive(liveVal, badge.digits)}
            {badge.unit ? ` ${badge.unit}` : ""}
          </div>
        )}
        <NodeMarks elementId={element.id} />
        {/* the status colour with the diagram's colour inside: at least 3:1
            both ways in both themes (contrast.test.ts) */}
        {issue && (
          <span
            className={`absolute -right-1.5 -top-1.5 z-20 flex h-3.5 w-3.5 items-center justify-center rounded-full text-[color:var(--ss-panel)] shadow ${
              issue.level === "error" ? "bg-[color:var(--ss-err)]" : "bg-[color:var(--ss-warn)]"
            }`}
            title={issue.text}
          >
            <AlertTriangle size={9} strokeWidth={2.5} />
          </span>
        )}
        {/* eslint-disable-next-line react-hooks/static-components -- componentIcon picks from a fixed module-level registry, so Icon is the same component every render */}
        <Icon
          size={isSub ? 26 : 30}
          strokeWidth={1.6}
          className={isSub ? "text-[color:var(--ss-accent)]" : undefined}
          style={isSub ? undefined : { color: domainColor }}
        />
        {isSub && (
          <span className="absolute bottom-0.5 right-1 text-meta font-semibold uppercase tracking-wide text-[color:var(--ss-accent)]">
            SYS
          </span>
        )}
        {(Object.keys(bySide) as PortSide[]).map((sd) =>
          bySide[sd].map((p) => (
            <PhysicalPort
              key={p.id}
              port={p}
              side={sd}
              frac={fracOf(p)}
              boxRef={boxRef}
              onCycle={onCycle}
              onDragPreview={onDragPreview}
              onDragCommit={onDragCommit}
            />
          )),
        )}
        <div className="ss-node-label pointer-events-none absolute left-1/2 top-full mt-1 w-[96px] -translate-x-1/2 text-center text-meta leading-tight">
          <span
            className={`rounded px-1 ${selected ? "font-semibold text-[color:var(--ss-accent)]" : "text-[color:var(--ss-node-icon)]"}`}
            style={{ background: "color-mix(in srgb, var(--ss-panel) 78%, transparent)" }}
          >
            {element.label}
          </span>
        </div>
      </div>
    </>
  );
});
