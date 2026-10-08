// Bulk wiring for Data Bus Connections (UX-15): link one output to the same
// input on every part of a type ("connect to all Brakes"), or pair outputs
// and inputs whose names match. The planners only read the project and say
// which links they would make and which inputs they leave alone, and why, so
// the panel can show the list before anything changes; applyBulkLinks then
// makes them all as one undo step.
import { portsOf, uid, useProjectStore } from "./store/projectStore";
import type { ComponentDef, PortDef, Project } from "./types";

/** A signal port of a part. */
export interface BusEnd {
  elementId: string;
  /** the part's library type (mech.brake) */
  defId: string;
  /** the part's name */
  label: string;
  port: PortDef;
  /** "Front Brake · Brake Command" */
  name: string;
}

/** What feeds an input: an output, by a data bus link (`linkId`) or by a
 *  wire on the diagram (`linkId` null). */
export interface Feed {
  from: BusEnd;
  linkId: string | null;
}

export interface BusIndex {
  inputs: BusEnd[];
  outputs: BusEnd[];
  /** input key → what feeds it (an input made before 0.3 can have two) */
  feeds: Map<string, Feed[]>;
}

/** A link to make; `replaces` are the data bus links it takes the place of. */
export interface PlannedLink {
  from: BusEnd;
  to: BusEnd;
  replaces: string[];
}

/** An input a plan leaves alone, and why. */
export interface Skipped {
  to: BusEnd;
  why: string;
}

export interface BulkPlan {
  links: PlannedLink[];
  skipped: Skipped[];
}

/** Inputs of one name on every part of one type: what "connect to all" fills. */
export interface InputGroup {
  key: string;
  defId: string;
  /** the type's name in the library (Brake) */
  typeName: string;
  portName: string;
  inputs: BusEnd[];
}

export const keyOf = (e: { elementId: string; port: { id: string } }) => `${e.elementId}:${e.port.id}`;

/** A name as the matcher compares it: "Brake Command", "brake_command" and
 *  "BrakeCommand" are the same name. */
export const normName = (name: string) => name.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");

const unitOf = (p: PortDef) => (p.unitGroup && p.unitGroup !== "No Unit" ? p.unitGroup : null);

/** Every signal port of the project and what feeds each input. */
export function busIndex(project: Project, libraryById: Record<string, ComponentDef>): BusIndex {
  const ends = new Map<string, BusEnd>();
  for (const el of project.systems.flatMap((s) => s.elements)) {
    for (const port of portsOf(el, libraryById)) {
      if (port.kind !== "signal") continue;
      ends.set(`${el.id}:${port.id}`, {
        elementId: el.id,
        defId: el.componentDefId,
        label: el.label,
        port,
        name: `${el.label} · ${port.name}`,
      });
    }
  }
  const feeds = new Map<string, Feed[]>();
  const feed = (e1: string, p1: string, e2: string, p2: string, linkId: string | null) => {
    const a = ends.get(`${e1}:${p1}`);
    const b = ends.get(`${e2}:${p2}`);
    if (!a || !b || a.port.direction === b.port.direction) return; // passes no data
    const [from, to] = a.port.direction === "output" ? [a, b] : [b, a];
    if (from.port.direction !== "output" || to.port.direction !== "input") return;
    const k = keyOf(to);
    feeds.set(k, [...(feeds.get(k) ?? []), { from, linkId }]);
  };
  for (const d of project.dataBusConnections) feed(d.element1Id, d.port1Id, d.element2Id, d.port2Id, d.id);
  for (const s of project.systems)
    for (const c of s.connections) feed(c.sourceElementId, c.sourcePortId, c.targetElementId, c.targetPortId, null);
  const all = [...ends.values()].sort((x, y) => x.name.localeCompare(y.name));
  return {
    inputs: all.filter((e) => e.port.direction === "input"),
    outputs: all.filter((e) => e.port.direction === "output"),
    feeds,
  };
}

/** The inputs that two or more parts of one type have (every Brake's Brake
 *  Command), by the type's name and then the input's. */
export function inputGroups(index: BusIndex, libraryById: Record<string, ComponentDef>): InputGroup[] {
  const groups = new Map<string, InputGroup>();
  for (const e of index.inputs) {
    const key = `${e.defId}\u0000${normName(e.port.name)}`;
    const g = groups.get(key);
    if (g) g.inputs.push(e);
    else
      groups.set(key, {
        key,
        defId: e.defId,
        typeName: libraryById[e.defId]?.name ?? e.defId,
        portName: e.port.name,
        inputs: [e],
      });
  }
  return [...groups.values()]
    .filter((g) => g.inputs.length > 1)
    .sort((a, b) => a.typeName.localeCompare(b.typeName) || a.portName.localeCompare(b.portName));
}

const sourceText = (feeds: Feed[]) =>
  feeds.map((f) => f.from.name + (f.linkId ? "" : " (a wire on the diagram)")).join(" and ");

/** Link `source` to every input of `group`. An input that already takes
 *  this source is left as it is; one with another source keeps it unless
 *  `replace` (a wire drawn on the diagram is always kept: delete it there). */
export function planConnectToAll(
  index: BusIndex,
  source: BusEnd,
  group: InputGroup,
  { replace = false }: { replace?: boolean } = {},
): BulkPlan {
  const plan: BulkPlan = { links: [], skipped: [] };
  for (const to of group.inputs) {
    const feeds = index.feeds.get(keyOf(to)) ?? [];
    if (to.elementId === source.elementId) {
      plan.skipped.push({ to, why: "it is on the source's own part" });
    } else if (feeds.length === 1 && keyOf(feeds[0].from) === keyOf(source)) {
      plan.skipped.push({ to, why: "already connected to it" });
    } else if (feeds.length && !replace) {
      plan.skipped.push({ to, why: `keeps its source, ${sourceText(feeds)}` });
    } else if (feeds.some((f) => !f.linkId)) {
      plan.skipped.push({ to, why: `wired on the diagram to ${sourceText(feeds.filter((f) => !f.linkId))}` });
    } else {
      plan.links.push({ from: source, to, replaces: feeds.map((f) => f.linkId!) });
    }
  }
  return plan;
}

/** Pair each input that has no source with the one output of the same name
 *  ("Brake Command" ← Driver · Brake Command; a Script's "vehicle_speed" ←
 *  Vehicle · Vehicle Speed), or the one whose part and port together are the
 *  name (a Script's "Battery SOC" ← Battery · SOC). An output on the input's
 *  own part, or with a different unit, is never paired; an input that more
 *  than one output matches is listed, for you to pick its source. With
 *  `elementId`, only links to or from that part. Inputs that have a source
 *  keep it. */
export function planMatchingNames(index: BusIndex, { elementId }: { elementId?: string } = {}): BulkPlan {
  const plan: BulkPlan = { links: [], skipped: [] };
  const byPort = new Map<string, BusEnd[]>();
  const byFull = new Map<string, BusEnd[]>();
  const add = (m: Map<string, BusEnd[]>, k: string, e: BusEnd) => m.set(k, [...(m.get(k) ?? []), e]);
  for (const o of index.outputs) {
    add(byPort, normName(o.port.name), o);
    add(byFull, normName(`${o.label} ${o.port.name}`), o);
  }
  for (const to of index.inputs) {
    if (index.feeds.get(keyOf(to))?.length) continue;
    const name = normName(to.port.name);
    if (!name) continue;
    const others = (list: BusEnd[] = []) => list.filter((o) => o.elementId !== to.elementId);
    const full = others(byFull.get(name));
    const found = full.length ? full : others(byPort.get(name));
    // another part's input is in scope when the part's own output matches
    // it (then the outputs elsewhere that match too still make it a choice)
    if (!found.length || (elementId && to.elementId !== elementId && !found.some((o) => o.elementId === elementId)))
      continue;
    const unit = unitOf(to.port);
    const fits = found.filter((o) => !unit || !unitOf(o.port) || unitOf(o.port) === unit);
    if (fits.length === 1 && elementId && ![to.elementId, fits[0].elementId].includes(elementId)) continue;
    if (fits.length === 1) plan.links.push({ from: fits[0], to, replaces: [] });
    else if (fits.length > 1)
      plan.skipped.push({ to, why: `${fits.length} outputs match: ${fits.map((o) => o.name).join(", ")}` });
    else
      plan.skipped.push({
        to,
        why: `${found.map((o) => o.name).join(", ")} ${found.length > 1 ? "have" : "has"} another unit (${found
          .map((o) => o.port.unitGroup)
          .join(", ")}, not ${to.port.unitGroup})`,
      });
  }
  return plan;
}

/** Make the planned links as one undo step: one Undo takes them all back.
 *  The project store has no call for several links at once (addDataBus
 *  makes one link per undo step), so this records the step with its
 *  beginHistory and sets the project once. Each link is checked again
 *  against the project as it is now: a link whose ports are gone, that
 *  would give an input a second source, or that exists already is left
 *  out. Answers how many links were made. */
export function applyBulkLinks(links: PlannedLink[], what: string): number {
  const store = useProjectStore.getState();
  const { project, libraryById } = store;
  if (!project) return 0;
  const index = busIndex(project, libraryById);
  const ends = new Map([...index.inputs, ...index.outputs].map((e) => [keyOf(e), e]));
  const drop = new Set<string>();
  const taken = new Set<string>();
  const ok = links.filter(({ from, to, replaces }) => {
    const f = ends.get(keyOf(from));
    const t = ends.get(keyOf(to));
    if (f?.port.direction !== "output" || t?.port.direction !== "input" || taken.has(keyOf(to))) return false;
    const feeds = index.feeds.get(keyOf(to)) ?? [];
    if (feeds.some((x) => !x.linkId || !replaces.includes(x.linkId))) return false;
    taken.add(keyOf(to));
    replaces.forEach((id) => drop.add(id));
    return true;
  });
  if (!ok.length) return 0;
  const next = structuredClone(project);
  next.dataBusConnections = next.dataBusConnections.filter((d) => !drop.has(d.id));
  for (const { from, to } of ok)
    next.dataBusConnections.push({
      id: uid("dbc"),
      element1Id: from.elementId,
      port1Id: from.port.id,
      element2Id: to.elementId,
      port2Id: to.port.id,
    });
  store.beginHistory();
  useProjectStore.setState({ project: next, dirty: true });
  const n = ok.length;
  store.log(
    "info",
    `Data bus: ${n} link${n === 1 ? "" : "s"} connected (${what})` +
      (drop.size ? `, replacing ${drop.size}` : "") +
      ". One Undo takes them all back.",
  );
  return n;
}
