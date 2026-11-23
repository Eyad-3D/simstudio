// Where a part's ports sit on its outline, shared by the part on the diagram
// (ElementNode) and the wire router (route.ts), and where a moved port may go.
import type { ComponentDef, ElementInstance, PortDef, PortKind, PortSide, Project, SystemNode } from "../../types";
import type { Box, PartIn, Pin, WireIn } from "./route";

export const DEFAULT_NODE_WIDTH = 92;

/** A port's side of its part and its place along that side, 0 to 1. */
export type Place = { side: PortSide; frac: number };

export const SIDES: PortSide[] = ["left", "top", "right", "bottom"];

/** The physical ports (signals are wired in Data Bus Connections). */
export const physicalPorts = (def: ComponentDef): PortDef[] => def.ports.filter((p) => p.kind !== "signal");

/** The ports drawn on the diagram: the physical ones the user has not hidden. */
export const drawnPorts = (el: ElementInstance, def: ComponentDef): PortDef[] =>
  physicalPorts(def).filter((p) => !el.hiddenPorts?.includes(p.id));

/** The junctions, whose parts may have as many terminals as the user needs
 *  (`terminals` on the part), and what a terminal is called. A node keeps
 *  at least two. */
export const JUNCTIONS: Record<string, string> = { "electric.node": "Terminal", "mech.node": "Flange" };

/** A part's definition as the project has it: a junction with its own
 *  terminals (each like the library's first) in the place of the library's. */
export function partDef(el: ElementInstance, def: ComponentDef): ComponentDef {
  if (!el.terminals || !JUNCTIONS[def.id]) return def;
  const lib = physicalPorts(def);
  const own = el.terminals.map(
    (id): PortDef =>
      lib.find((p) => p.id === id) ?? { ...lib[0], id, name: `${JUNCTIONS[def.id]} ${id.slice(1)}`, side: "right" },
  );
  return { ...def, ports: [...own, ...def.ports.filter((p) => p.kind === "signal")] };
}

/** The diagram's wired ports, as "element:port". */
export const wiredPorts = (system: SystemNode): Set<string> =>
  new Set(
    system.connections.flatMap((c) => [`${c.sourceElementId}:${c.sourcePortId}`, `${c.targetElementId}:${c.targetPortId}`]),
  );

/** The part as drawn: a hidden port that has a wire after all (a file
 *  written elsewhere) is drawn, so that its wire has an end. */
export function asDrawn(el: ElementInstance, wired: Set<string>): ElementInstance {
  const keep = el.hiddenPorts?.filter((p) => !wired.has(`${el.id}:${p}`));
  return keep && keep.length !== el.hiddenPorts!.length ? { ...el, hiddenPorts: keep } : el;
}

/** Whether a port has a wire or a signal link. */
export function portInUse(project: Project, elementId: string, portId: string): boolean {
  return (
    project.systems.some((s) =>
      s.connections.some(
        (c) =>
          (c.sourceElementId === elementId && c.sourcePortId === portId) ||
          (c.targetElementId === elementId && c.targetPortId === portId),
      ),
    ) ||
    project.dataBusConnections.some(
      (d) => (d.element1Id === elementId && d.port1Id === portId) || (d.element2Id === elementId && d.port2Id === portId),
    )
  );
}

/** A junction's terminals after adding one: the next number after its highest. */
export function withTerminal(el: ElementInstance, def: ComponentDef): string[] {
  const ids = physicalPorts(partDef(el, def)).map((p) => p.id);
  const next = Math.max(...ids.map((id) => Number(id.slice(1)) || 0)) + 1;
  return [...ids, `${ids[0][0]}${next}`];
}

/** A port's place when it comes onto a part whose ports were moved (shown
 *  again, or added): a free one, on its own side if it has room; null when
 *  the part's ports are where the library puts them (they spread evenly). */
export function freePlace(el: ElementInstance, def: ComponentDef, portId: string): Place | null {
  if (!Object.keys(el.portOffsets ?? {}).length) return null;
  const own = (def.ports.find((p) => p.id === portId)?.side as PortSide) || "right";
  for (const side of [own, ...SIDES.filter((s) => s !== own)]) {
    const place = movedPlace(el, def, portId, side, 0.5);
    if (place) return place;
  }
  return null;
}

/** Every drawn port's place: the one saved with the part, else spread evenly
 *  along the side the library gives it. */
export function portPlaces(el: ElementInstance, def: ComponentDef): Record<string, Place> {
  const bySide: Record<PortSide, PortDef[]> = { left: [], right: [], top: [], bottom: [] };
  for (const p of drawnPorts(el, def)) bySide[el.portSides?.[p.id] ?? ((p.side as PortSide) || "left")].push(p);
  const out: Record<string, Place> = {};
  for (const side of SIDES)
    bySide[side].forEach((p, i, list) => {
      const stored = el.portOffsets?.[p.id];
      out[p.id] = { side, frac: typeof stored === "number" ? stored : (i + 1) / (list.length + 1) };
    });
  return out;
}

/** A part's size on the diagram: the one the user gave it, else wide enough
 *  for its name and tall enough for its rows of ports. */
export function nodeSize(el: ElementInstance, places: Record<string, Place>): { width: number; height: number } {
  const count = (side: PortSide) => Object.values(places).filter((p) => p.side === side).length;
  const rows = Math.max(count("left"), count("right"), 1);
  return {
    width: el.size?.width ?? DEFAULT_NODE_WIDTH,
    height: el.size?.height ?? Math.max(54, rows * 18 + 18),
  };
}

/** The least distance between two ports on a side, in diagram units (a port is 9 wide). */
export const PORT_GAP = 12;

/** Where a port let go near `want` (0 to 1) on a side of `length` goes: the
 *  nearest sixth of the side, else twelfth, at least PORT_GAP from the side's
 *  other ports (`others`); null when no such place is left. */
export function portSpot(want: number, others: number[], length: number): number | null {
  for (const n of [6, 12]) {
    let best: number | null = null;
    for (let k = 1; k < n; k++) {
      const f = k / n;
      if (others.some((o) => Math.abs(o - f) * length < PORT_GAP)) continue;
      // (of two as near, the first along the side)
      if (best === null || Math.abs(f - want) < Math.abs(best - want) - 1e-9) best = f;
    }
    if (best !== null) return best;
  }
  return null;
}

/** The place for one port moved to `side` near `want`, kept clear of the
 *  part's other ports; null when that side is full. */
export function movedPlace(
  el: ElementInstance,
  def: ComponentDef,
  portId: string,
  side: PortSide,
  want: number,
): Place | null {
  const places = portPlaces(el, def);
  const others = Object.entries(places)
    .filter(([id, p]) => id !== portId && p.side === side)
    .map(([, p]) => p.frac);
  const size = nodeSize(el, places);
  const frac = portSpot(want, others, side === "top" || side === "bottom" ? size.width : size.height);
  return frac === null ? null : { side, frac };
}

/** The saved places after moving one port: every drawn port of the part is
 *  written down as it is now, so moving one never shifts another. */
export function placesAfterMove(
  el: ElementInstance,
  def: ComponentDef,
  portId: string,
  place: Place,
): { portSides: Record<string, PortSide>; portOffsets: Record<string, number> } {
  const places = { ...portPlaces(el, def), [portId]: place };
  const portSides: Record<string, PortSide> = {};
  const portOffsets: Record<string, number> = {};
  for (const [id, p] of Object.entries(places)) {
    portSides[id] = p.side;
    portOffsets[id] = Math.round(p.frac * 1e4) / 1e4;
  }
  return { portSides, portOffsets };
}

/** A port's point on the diagram, on its part's outline. */
export function pinPoint(box: Box, place: Place): Pin {
  const { side, frac } = place;
  if (side === "left") return { x: box.x, y: box.y + frac * box.h, side };
  if (side === "right") return { x: box.x + box.w, y: box.y + frac * box.h, side };
  if (side === "top") return { x: box.x + frac * box.w, y: box.y, side };
  return { x: box.x + frac * box.w, y: box.y + box.h, side };
}

// A part's name hangs under it at 11 px on the screen whatever the zoom
// (.ss-node-label), so it is bigger on a zoomed-out diagram: kept clear as
// it is at about 70 %, where the examples open.
const LABEL_SCALE = 1.4;

/** The box a part's name takes under it (an estimate from its length). */
export function labelBox(text: string, box: Box): Box {
  const textW = text.length * 6 + 8;
  const lines = Math.max(1, Math.ceil(textW / 96));
  const w = Math.min(96, textW) * LABEL_SCALE;
  return { x: box.x + box.w / 2 - w / 2, y: box.y + box.h + 4, w, h: lines * 13 * LABEL_SCALE };
}

/** The parts and wires of a diagram, as the router takes them. `sizeOf`
 *  gives a part's measured size; `shown` says which wires are drawn. */
export function routingInput(
  system: SystemNode,
  libraryById: Record<string, ComponentDef>,
  sizeOf: (el: ElementInstance) => { width: number; height: number } | undefined = () => undefined,
  shown: (kind: PortKind) => boolean = () => true,
): { parts: PartIn[]; wires: WireIn[] } {
  const geo: Record<string, { box: Box; places: Record<string, Place>; def: ComponentDef }> = {};
  const parts: PartIn[] = [];
  const wired = wiredPorts(system);
  for (const raw of system.elements) {
    const lib = libraryById[raw.componentDefId];
    if (!lib) continue;
    const el = asDrawn(raw, wired);
    const def = partDef(el, lib);
    const places = portPlaces(el, def);
    const size = el.size ?? sizeOf(el) ?? nodeSize(el, places);
    const box = { x: el.position.x, y: el.position.y, w: size.width, h: size.height };
    geo[el.id] = { box, places, def };
    parts.push({ box, label: labelBox(el.label, box) });
  }
  // joined ports are one net: their wires may share a stretch
  const root: Record<string, string> = {};
  const find = (k: string): string => {
    while (root[k] !== k) k = root[k];
    return k;
  };
  const wires: WireIn[] = [];
  const ends: [string, Pin, Pin, string, string][] = [];
  for (const c of system.connections) {
    const a = geo[c.sourceElementId];
    const b = geo[c.targetElementId];
    const pa = a?.places[c.sourcePortId];
    const pb = b?.places[c.targetPortId];
    if (!a || !b || !pa || !pb) continue;
    const kind = a.def.ports.find((p) => p.id === c.sourcePortId)?.kind;
    if (!kind || !shown(kind)) continue;
    const ka = `${c.sourceElementId}:${c.sourcePortId}`;
    const kb = `${c.targetElementId}:${c.targetPortId}`;
    root[ka] ??= ka;
    root[kb] ??= kb;
    root[find(ka)] = find(kb);
    ends.push([c.id, pinPoint(a.box, pa), pinPoint(b.box, pb), ka, kb]);
  }
  for (const [id, from, to, ka] of ends) wires.push({ id, from, to, net: find(ka) });
  return { parts, wires };
}
