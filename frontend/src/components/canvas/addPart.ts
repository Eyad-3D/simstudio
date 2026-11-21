// Adding a part from the diagram (the add-a-part box): which parts a wire
// dragged from a port may reach, and which of their ports it goes to.
// Plain functions, so they unit-test without a browser.
import { searchLibrary, searchTerms } from "../../componentSearch";
import type { ComponentDef, PortDef } from "../../types";

/** Whether two ports may be joined by a wire on the diagram: the canvas's
 *  own rule (TopologyCanvas isValidConnection), the same kind and not a
 *  signal (signals are wired in Data Bus Connections). */
export const portsJoin = (a: Pick<PortDef, "kind">, b: Pick<PortDef, "kind">): boolean =>
  a.kind === b.kind && a.kind !== "signal";

/**
 * The port of a new part `def` that a wire from port `from` goes to, or
 * null when it has none: of its ports the wire may join, the one with the
 * same id (a battery's negative terminal to a motor's negative terminal,
 * not to its first electrical port), else the first in the library's order.
 * Every port of a new part is free.
 */
export function wirePort(def: ComponentDef, from: PortDef): string | null {
  const fit = def.ports.filter((p) => portsJoin(p, from));
  return (fit.find((p) => p.id === from.id) ?? fit[0])?.id ?? null;
}

/** Where a part chosen in the box goes: centred on the spot where the box
 *  opened (a new part is about 92 × 54 px, as a part dropped from the
 *  Components panel), on the grid when Snap is on (`grid` its step). */
export function partSpot(at: { x: number; y: number }, grid: number | null): { x: number; y: number } {
  const on = (v: number) => (grid ? Math.round(v / grid) * grid : v);
  return { x: on(at.x - 46), y: on(at.y - 27) };
}

/** The parts the box lists for a search: those the search finds (the
 *  Components panel's matching), and with a dragged wire only those it may
 *  connect to. Enter takes the first, so parts whose name holds every word
 *  come first (a name that starts with the search before the rest), then
 *  those found by their description or other names; each group keeps the
 *  library's order. "brake" gives Brake first, not Driver, whose
 *  description mentions braking. */
export function partsFor(library: ComponentDef[], query: string, from?: PortDef | null): ComponentDef[] {
  const found = searchLibrary(library, query).filter((d) => !from || wirePort(d, from) !== null);
  const terms = searchTerms(query);
  if (!terms.length) return found;
  const q = terms.join(" ");
  const rank = (d: ComponentDef) => {
    const name = d.name.toLowerCase();
    return name.startsWith(q) ? 0 : terms.every((t) => name.includes(t)) ? 1 : 2;
  };
  return found.map((d, i) => ({ d, i, r: rank(d) })).sort((a, b) => a.r - b.r || a.i - b.i).map((x) => x.d);
}
