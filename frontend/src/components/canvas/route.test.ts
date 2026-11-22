import bev from "../../../../backend/projects/bev-car.json";
import hybrid from "../../../../backend/projects/hybrid-car.json";
import aero from "../../../../backend/projects/aero-bev.json";
import fs from "../../../../backend/projects/fs-electric.json";
import { describe, expect, it } from "vitest";
import libraryJson from "../../data/componentLibrary.json";
import type { ComponentDef, Project } from "../../types";
import { routingInput } from "./portPlaces";
import { CLEAR, GAP, fitEnds, roundedPath, routeWires, type Box, type PartIn, type Pt, type WireIn } from "./route";

const libraryById = Object.fromEntries((libraryJson as unknown as { components: ComponentDef[] }).components.map((d) => [d.id, d]));
const example = (name: string) =>
  ({ "bev-car": bev, "hybrid-car": hybrid, "aero-bev": aero, "fs-electric": fs })[name] as unknown as Project;

type Seg = { a: Pt; b: Pt; wire: string; net: string };
const segments = (routes: Record<string, Pt[]>, wires: WireIn[]): Seg[] =>
  wires.flatMap((w) => routes[w.id].slice(1).map((b, k) => ({ a: routes[w.id][k], b, wire: w.id, net: w.net })));

/** Length of a segment inside a box (strictly inside). */
function inside(s: Seg, b: Box): number {
  const x0 = Math.max(Math.min(s.a.x, s.b.x), b.x);
  const x1 = Math.min(Math.max(s.a.x, s.b.x), b.x + b.w);
  const y0 = Math.max(Math.min(s.a.y, s.b.y), b.y);
  const y1 = Math.min(Math.max(s.a.y, s.b.y), b.y + b.h);
  if (s.a.x === s.b.x) return s.a.x > b.x && s.a.x < b.x + b.w ? Math.max(0, y1 - y0) : 0;
  return s.a.y > b.y && s.a.y < b.y + b.h ? Math.max(0, x1 - x0) : 0;
}

/** Collinear stretches of two wires of different nets that lie on each other. */
function overlaps(segs: Seg[]): number {
  let n = 0;
  for (let i = 0; i < segs.length; i++)
    for (let k = i + 1; k < segs.length; k++) {
      const s = segs[i];
      const t = segs[k];
      if (s.net === t.net) continue;
      const sh = s.a.y === s.b.y;
      const th = t.a.y === t.b.y;
      if (sh !== th) continue;
      if (sh && s.a.y === t.a.y) {
        const lo = Math.max(Math.min(s.a.x, s.b.x), Math.min(t.a.x, t.b.x));
        const hi = Math.min(Math.max(s.a.x, s.b.x), Math.max(t.a.x, t.b.x));
        if (hi - lo > 0.5) n++;
      } else if (!sh && s.a.x === t.a.x) {
        const lo = Math.max(Math.min(s.a.y, s.b.y), Math.min(t.a.y, t.b.y));
        const hi = Math.min(Math.max(s.a.y, s.b.y), Math.max(t.a.y, t.b.y));
        if (hi - lo > 0.5) n++;
      }
    }
  return n;
}

/** Proper crossings between wires of different nets. */
function crossings(segs: Seg[]): number {
  let n = 0;
  for (let i = 0; i < segs.length; i++)
    for (let k = i + 1; k < segs.length; k++) {
      const s = segs[i];
      const t = segs[k];
      if (s.net === t.net) continue;
      const [hz, vt] = s.a.y === s.b.y && t.a.x === t.b.x ? [s, t] : t.a.y === t.b.y && s.a.x === s.b.x ? [t, s] : [null, null];
      if (!hz || !vt) continue;
      const x = vt.a.x;
      const y = hz.a.y;
      if (x > Math.min(hz.a.x, hz.b.x) && x < Math.max(hz.a.x, hz.b.x) && y > Math.min(vt.a.y, vt.b.y) && y < Math.max(vt.a.y, vt.b.y)) n++;
    }
  return n;
}

const orthogonal = (pts: Pt[]) => pts.slice(1).every((b, k) => b.x === pts[k].x || b.y === pts[k].y);

const box = (x: number, y: number, w = 90, h = 54): PartIn => ({ box: { x, y, w, h } });

describe("the wire router", () => {
  it("goes round a part in the way, in straight stretches, leaving each port straight out", () => {
    const parts = [box(0, 0), box(200, 0), box(400, 0)];
    const wires: WireIn[] = [{ id: "w", from: { x: 90, y: 27, side: "right" }, to: { x: 400, y: 27, side: "left" }, net: "n" }];
    const r = routeWires(parts, wires);
    const pts = r.w;
    expect(orthogonal(pts)).toBe(true);
    expect(pts[0]).toEqual({ x: 90, y: 27 });
    expect(pts[1].y).toBe(27); // out to the right first
    expect(pts[1].x).toBeGreaterThan(90);
    expect(pts[pts.length - 2].y).toBe(27); // in from the left
    for (const s of segments(r, wires)) {
      const b = parts[1].box;
      expect(inside(s, { x: b.x - CLEAR + 1, y: b.y - CLEAR + 1, w: b.w + 2 * CLEAR - 2, h: b.h + 2 * CLEAR - 2 })).toBe(0);
    }
  });

  it("keeps wires running side by side apart, never on each other", () => {
    const parts = [box(0, 0, 90, 90), box(400, 0, 90, 90), box(200, 200)];
    const wires: WireIn[] = [
      { id: "a", from: { x: 90, y: 30, side: "right" }, to: { x: 400, y: 60, side: "left" }, net: "1" },
      { id: "b", from: { x: 90, y: 60, side: "right" }, to: { x: 400, y: 30, side: "left" }, net: "2" },
      { id: "c", from: { x: 245, y: 200, side: "top" }, to: { x: 445, y: 90, side: "bottom" }, net: "3" },
    ];
    const r = routeWires(parts, wires);
    const segs = segments(r, wires);
    expect(overlaps(segs)).toBe(0);
    // parallel stretches of different wires that run alongside: at least GAP apart
    for (const s of segs)
      for (const t of segs) {
        if (s.net === t.net) continue;
        if (s.a.x === s.b.x && t.a.x === t.b.x && s.a.x !== t.a.x) {
          const lo = Math.max(Math.min(s.a.y, s.b.y), Math.min(t.a.y, t.b.y));
          const hi = Math.min(Math.max(s.a.y, s.b.y), Math.max(t.a.y, t.b.y));
          if (hi - lo > 1) expect(Math.abs(s.a.x - t.a.x)).toBeGreaterThanOrEqual(GAP);
        }
      }
  });

  it("goes round rather than across another wire when a fair detour exists", () => {
    // a wire along the top row, and one from below that could cut across it
    // or go round the right-hand part
    const parts = [box(0, 100), box(300, 100), box(150, 300)];
    const wires: WireIn[] = [
      { id: "top", from: { x: 90, y: 127, side: "right" }, to: { x: 300, y: 127, side: "left" }, net: "1" },
      { id: "up", from: { x: 240, y: 327, side: "right" }, to: { x: 390, y: 127, side: "right" }, net: "2" },
    ];
    const r = routeWires(parts, wires);
    expect(crossings(segments(r, wires))).toBe(0);
  });

  it("crosses at a right angle when it must", () => {
    // four parts in a cross: the two wires cannot avoid each other
    const parts = [box(0, 200), box(400, 200), box(200, 0), box(200, 400)];
    const wires: WireIn[] = [
      { id: "h", from: { x: 90, y: 227, side: "right" }, to: { x: 400, y: 227, side: "left" }, net: "1" },
      { id: "v", from: { x: 245, y: 54, side: "bottom" }, to: { x: 245, y: 400, side: "top" }, net: "2" },
    ];
    const r = routeWires(parts, wires);
    expect(orthogonal(r.h) && orthogonal(r.v)).toBe(true);
    expect(crossings(segments(r, wires))).toBeLessThanOrEqual(1);
  });

  it("gives the same routes every time", () => {
    const p = example("hybrid-car");
    const { parts, wires } = routingInput(p.systems[0], libraryById);
    expect(routeWires(parts, wires)).toEqual(routeWires(parts, [...wires]));
  });

  it("routes the largest example quickly, through no part, with no wire on another", () => {
    for (const name of ["bev-car", "hybrid-car", "aero-bev", "fs-electric"]) {
      const p = example(name);
      const { parts, wires } = routingInput(p.systems[0], libraryById);
      routeWires(parts, wires); // warm up
      const t0 = performance.now();
      const runs = 5;
      let r: Record<string, Pt[]> = {};
      for (let k = 0; k < runs; k++) r = routeWires(parts, wires);
      const ms = (performance.now() - t0) / runs;
      const segs = segments(r, wires);
      const through = segs.reduce((n, s) => n + parts.reduce((m, q) => m + inside(s, q.box), 0), 0);
      const length = segs.reduce((n, s) => n + Math.abs(s.b.x - s.a.x) + Math.abs(s.b.y - s.a.y), 0);
      const bends = wires.reduce((n, w) => n + r[w.id].length - 2, 0);
      console.log(
        `${name}: ${parts.length} parts, ${wires.length} wires, ${ms.toFixed(1)} ms, ${crossings(segs)} crossings, ` +
          `${overlaps(segs)} overlaps, ${through} units through parts, length ${Math.round(length)}, ${bends} bends`,
      );
      expect(overlaps(segs)).toBe(0);
      expect(through).toBe(0);
      expect(ms).toBeLessThan(250);
      for (const w of wires) expect(orthogonal(r[w.id])).toBe(true);
    }
  });
});

describe("drawing a routed wire", () => {
  it("rounds its corners, less where a stretch is short", () => {
    expect(roundedPath([{ x: 0, y: 0 }, { x: 20, y: 0 }, { x: 20, y: 4 }])).toBe("M0,0 L18,0 Q20,0 20,2 L20,4");
  });

  it("puts the ends on the ports' dots, or gives up when a part has moved", () => {
    const pts = [{ x: 90, y: 27 }, { x: 102, y: 27 }, { x: 102, y: 80 }, { x: 188, y: 80 }, { x: 200, y: 80 }];
    expect(fitEnds(pts, { x: 94.5, y: 27.4 }, { x: 195.5, y: 80 })).toEqual([
      { x: 94.5, y: 27.4 },
      { x: 102, y: 27.4 },
      { x: 102, y: 80 },
      { x: 188, y: 80 },
      { x: 195.5, y: 80 },
    ]);
    expect(fitEnds(pts, { x: 94.5, y: 60 }, { x: 195.5, y: 80 })).toBeNull();
  });
});
