// The wire router: each wire on the diagram in horizontal and vertical
// stretches that leave and enter a port straight out from its side, keep clear
// of parts and their names, never lie on another wire and cross one only where
// going round would be much longer.
//
// How: a sparse grid of lines (each part's outline pushed out by a clear gap,
// with a few tracks a wire's width apart beyond it, the ports' lines, and the
// lines midway between parts), and on it a shortest-path search (A*) whose
// cost adds length, bends, crossings and running close to a part or another
// wire. Wires go shortest first; then each is routed again with all the others
// in place. No randomness: the same diagram gives the same routes.

export type Pt = { x: number; y: number };
export type Box = { x: number; y: number; w: number; h: number };
export type Side = "left" | "right" | "top" | "bottom";
export type Pin = { x: number; y: number; side: Side };
/** A wire between two ports; wires of one `net` (joined ports) may share a stretch. */
export type WireIn = { id: string; from: Pin; to: Pin; net: string };
/** A part's outline and the box of its name. */
export type PartIn = { box: Box; label?: Box };

/** The clear gap between a wire and a part, in diagram units. */
export const CLEAR = 12;
/** The gap between wires running side by side: about 7 px at the 60 % the
 *  examples open at, enough to tell two wires apart (8 was 4 to 5 px, and
 *  wires that close read as one line). */
export const GAP = 12;
const TRACKS = 3;

// costs, in diagram units of length
const BEND = 24;
const CROSS = 160; // crossing another wire: a detour up to this long is taken instead
const TOUCH = 600; // turning on another wire (it would look joined)
const OVERLAP = 800; // lying on another wire
// per unit running closer than GAP beside another wire: a run of 50 units costs
// a detour of 300 (higher made routes loop round and cross more)
const NEAR = 6;
const IN_CLEAR = 40; // per unit within a part's clear gap
const IN_PART = 400; // per unit through a part
const IN_LABEL = 8; // per unit over a part's name

const DIR: Record<Side, number> = { right: 0, left: 1, bottom: 2, top: 3 };
const DX = [1, -1, 0, 0];
const DY = [0, 0, 1, -1];
const REVERSE = [1, 0, 3, 2];

const r2 = (v: number) => Math.round(v * 2) / 2;

function lines(vals: number[]): number[] {
  const s = [...new Set(vals.map(r2))];
  return s.sort((a, b) => a - b);
}

function lowerIndex(arr: number[], v: number): number {
  let lo = 0;
  let hi = arr.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if (arr[m] < v) lo = m + 1;
    else hi = m;
  }
  return lo;
}

/** The stub: how far a wire runs straight out of each port before it may
 *  turn. Ports facing each other closer than two gaps meet halfway. */
function stubs(w: WireIn): [number, number] {
  const a = w.from;
  const b = w.to;
  const da = DIR[a.side];
  const db = DIR[b.side];
  if (REVERSE[da] === db) {
    const ahead = DX[da] * (b.x - a.x) + DY[da] * (b.y - a.y);
    if (ahead > 0 && ahead < 2 * CLEAR) return [ahead / 2, ahead / 2];
  }
  return [CLEAR, CLEAR];
}

const stubPoint = (p: Pin, len: number): Pt => ({ x: p.x + DX[DIR[p.side]] * len, y: p.y + DY[DIR[p.side]] * len });

/** Drop the points that lie on a straight line between their neighbours. */
function simplify(pts: Pt[]): Pt[] {
  const out: Pt[] = [];
  for (const p of pts) {
    const n = out.length;
    if (n && out[n - 1].x === p.x && out[n - 1].y === p.y) continue;
    if (n >= 2) {
      const a = out[n - 2];
      const b = out[n - 1];
      if ((a.x === b.x && b.x === p.x) || (a.y === b.y && b.y === p.y)) out[n - 1] = p;
      else out.push(p);
    } else out.push(p);
  }
  return out;
}

class Heap {
  f: number[] = [];
  s: number[] = [];
  push(f: number, s: number) {
    const F = this.f;
    const S = this.s;
    let i = F.length;
    F.push(f);
    S.push(s);
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (F[p] <= f) break;
      F[i] = F[p];
      S[i] = S[p];
      i = p;
    }
    F[i] = f;
    S[i] = s;
  }
  pop(): number {
    const F = this.f;
    const S = this.s;
    const top = S[0];
    const lf = F.pop()!;
    const ls = S.pop()!;
    const n = F.length;
    if (n) {
      let i = 0;
      for (;;) {
        let c = 2 * i + 1;
        if (c >= n) break;
        if (c + 1 < n && F[c + 1] < F[c]) c++;
        if (F[c] >= lf) break;
        F[i] = F[c];
        S[i] = S[c];
        i = c;
      }
      F[i] = lf;
      S[i] = ls;
    }
    return top;
  }
  get size() {
    return this.f.length;
  }
  minF() {
    return this.f[0];
  }
}

/** Route every wire; returns each wire's corner points from its first port to
 *  its second (the ports included). */
export function routeWires(parts: PartIn[], wires: WireIn[]): Record<string, Pt[]> {
  const out: Record<string, Pt[]> = {};
  if (wires.length === 0) return out;

  // --- the grid ---
  const xv: number[] = [];
  const yv: number[] = [];
  const edgesX: number[] = [];
  const edgesY: number[] = [];
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const { box: b, label } of parts) {
    const bottom = Math.max(b.y + b.h, label ? label.y + label.h : -Infinity);
    for (let k = 0; k < TRACKS; k++) {
      xv.push(b.x - CLEAR - k * GAP, b.x + b.w + CLEAR + k * GAP);
      yv.push(b.y - CLEAR - k * GAP, bottom + CLEAR + k * GAP);
    }
    if (label) xv.push(label.x - 2, label.x + label.w + 2);
    edgesX.push(b.x, b.x + b.w);
    edgesY.push(b.y, bottom);
    minX = Math.min(minX, b.x, label?.x ?? Infinity);
    maxX = Math.max(maxX, b.x + b.w, label ? label.x + label.w : -Infinity);
    minY = Math.min(minY, b.y);
    maxY = Math.max(maxY, bottom);
  }
  // midway between neighbouring part edges: a wire down the middle of a lane
  for (const [src, dst] of [
    [edgesX, xv],
    [edgesY, yv],
  ] as const) {
    const s = lines(src);
    for (let i = 0; i + 1 < s.length; i++) dst.push((s[i] + s[i + 1]) / 2);
  }
  const stubLen: Record<string, [number, number]> = {};
  for (const w of wires) {
    const st = stubs(w);
    stubLen[w.id] = st;
    for (const [p, len] of [
      [w.from, st[0]],
      [w.to, st[1]],
    ] as const) {
      const s = stubPoint(p, len);
      xv.push(s.x);
      yv.push(s.y);
      minX = Math.min(minX, s.x);
      maxX = Math.max(maxX, s.x);
      minY = Math.min(minY, s.y);
      maxY = Math.max(maxY, s.y);
    }
  }
  const far = CLEAR + TRACKS * GAP + 2 * CLEAR;
  xv.push(minX - far, maxX + far);
  yv.push(minY - far, maxY + far);
  const xs = lines(xv);
  const ys = lines(yv);
  const nx = xs.length;
  const ny = ys.length;
  const V = nx * ny;

  // extra cost per unit of each grid step: horizontal step v→v+1 at h[v],
  // vertical step v→v+nx at vt[v]
  const h = new Float32Array(V).fill(1);
  const vt = new Float32Array(V).fill(1);
  const paint = (b: Box, c: number) => {
    const x0 = b.x;
    const x1 = b.x + b.w;
    const y0 = b.y;
    const y1 = b.y + b.h;
    // horizontal steps: on a line strictly inside, with their middle inside
    for (let j = lowerIndex(ys, y0); j < ny && ys[j] < y1; j++) {
      if (ys[j] <= y0) continue;
      for (let i = Math.max(0, lowerIndex(xs, x0) - 1); i + 1 < nx && xs[i] < x1; i++) {
        const m = (xs[i] + xs[i + 1]) / 2;
        if (m > x0 && m < x1) h[j * nx + i] += c;
      }
    }
    for (let i = lowerIndex(xs, x0); i < nx && xs[i] < x1; i++) {
      if (xs[i] <= x0) continue;
      for (let j = Math.max(0, lowerIndex(ys, y0) - 1); j + 1 < ny && ys[j] < y1; j++) {
        const m = (ys[j] + ys[j + 1]) / 2;
        if (m > y0 && m < y1) vt[j * nx + i] += c;
      }
    }
  };
  for (const { box: b, label } of parts) {
    paint({ x: b.x - CLEAR, y: b.y - CLEAR, w: b.w + 2 * CLEAR, h: b.h + 2 * CLEAR }, IN_CLEAR);
    paint(b, IN_PART);
    if (label) paint({ x: label.x - 2, y: label.y - 2, w: label.w + 4, h: label.h + 4 }, IN_LABEL);
  }

  const xi = new Map(xs.map((x, i) => [x, i]));
  const yi = new Map(ys.map((y, j) => [y, j]));
  const vertexAt = (p: Pt) => (yi.get(r2(p.y)) ?? 0) * nx + (xi.get(r2(p.x)) ?? 0);
  const ptOf = (v: number): Pt => ({ x: xs[v % nx], y: ys[Math.floor(v / nx)] });

  // --- the wires already drawn: which net uses each step and each point,
  // across (H) and along (V); -1 none, -2 several ---
  const nets = new Map<string, number>();
  const netOf = (w: WireIn) => {
    if (!nets.has(w.net)) nets.set(w.net, nets.size);
    return nets.get(w.net)!;
  };
  const occH = new Int32Array(V);
  const occV = new Int32Array(V);
  const useH = new Int32Array(V);
  const useV = new Int32Array(V);
  const mark = (arr: Int32Array, i: number, net: number) => {
    const c = arr[i];
    arr[i] = c === -1 || c === net ? net : -2;
  };
  const paths = new Map<string, number[]>();
  const occupy = (path: number[], net: number) => {
    for (let k = 0; k + 1 < path.length; k++) {
      const a = path[k];
      const b = path[k + 1];
      if (b === a + 1 || b === a - 1) {
        mark(occH, Math.min(a, b), net);
        mark(useH, a, net);
        mark(useH, b, net);
      } else {
        mark(occV, Math.min(a, b), net);
        mark(useV, a, net);
        mark(useV, b, net);
      }
    }
  };
  const occupyAll = (except: string | null) => {
    occH.fill(-1);
    occV.fill(-1);
    useH.fill(-1);
    useV.fill(-1);
    for (const w of wires) {
      const p = paths.get(w.id);
      if (p && w.id !== except) occupy(p, netOf(w));
    }
  };
  const other = (c: number, net: number) => c !== -1 && c !== net;

  // --- the search, over (grid point, direction of travel) ---
  const S = V * 4;
  const g = new Float64Array(S);
  const came = new Int32Array(S);

  const search = (w: WireIn): number[] => {
    const net = netOf(w);
    const [la, lb] = stubLen[w.id];
    const start = vertexAt(stubPoint(w.from, la));
    const goal = vertexAt(stubPoint(w.to, lb));
    const d0 = DIR[w.from.side];
    const inward = REVERSE[DIR[w.to.side]];
    if (start === goal) return [start];
    const gx = goal % nx;
    const gy = Math.floor(goal / nx);
    const hEst = (v: number) => Math.abs(xs[v % nx] - xs[gx]) + Math.abs(ys[Math.floor(v / nx)] - ys[gy]);
    g.fill(Infinity);
    came.fill(-1);
    const heap = new Heap();
    const s0 = start * 4 + d0;
    g[s0] = 0;
    heap.push(hEst(start), s0);
    let best = Infinity;
    let bestState = -1;
    while (heap.size) {
      if (heap.minF() >= best) break;
      const s = heap.pop();
      const v = s >> 2;
      const d = s & 3;
      const gs = g[s];
      const i = v % nx;
      const j = (v - i) / nx;
      for (let nd = 0; nd < 4; nd++) {
        if (nd === REVERSE[d]) continue;
        let u: number;
        let step: number;
        let len: number;
        let horiz: boolean;
        if (nd === 0) {
          if (i + 1 >= nx) continue;
          u = v + 1;
          step = v;
          len = xs[i + 1] - xs[i];
          horiz = true;
        } else if (nd === 1) {
          if (i === 0) continue;
          u = v - 1;
          step = v - 1;
          len = xs[i] - xs[i - 1];
          horiz = true;
        } else if (nd === 2) {
          if (j + 1 >= ny) continue;
          u = v + nx;
          step = v;
          len = ys[j + 1] - ys[j];
          horiz = false;
        } else {
          if (j === 0) continue;
          u = v - nx;
          step = v - nx;
          len = ys[j] - ys[j - 1];
          horiz = false;
        }
        let c = len * (horiz ? h[step] : vt[step]);
        if (other(horiz ? occH[step] : occV[step], net)) c += OVERLAP + 20 * len;
        if (nd !== d) {
          c += BEND;
          if (other(useH[v], net) || other(useV[v], net)) c += TOUCH;
        } else if (other(horiz ? useV[v] : useH[v], net)) c += CROSS;
        // closer than GAP beside another wire, on any line that near, for
        // every unit of the stretch: two wires a few units apart read as one
        if (horiz) {
          for (let jj = j - 1; jj >= 0 && ys[j] - ys[jj] < GAP; jj--)
            if (other(occH[step + (jj - j) * nx], net)) c += NEAR * len;
          for (let jj = j + 1; jj < ny && ys[jj] - ys[j] < GAP; jj++)
            if (other(occH[step + (jj - j) * nx], net)) c += NEAR * len;
        } else {
          for (let ii = i - 1; ii >= 0 && xs[i] - xs[ii] < GAP; ii--) if (other(occV[step + ii - i], net)) c += NEAR * len;
          for (let ii = i + 1; ii < nx && xs[ii] - xs[i] < GAP; ii++) if (other(occV[step + ii - i], net)) c += NEAR * len;
        }
        const ns = u * 4 + nd;
        const gn = gs + c;
        if (gn >= g[ns]) continue;
        g[ns] = gn;
        came[ns] = s;
        if (u === goal) {
          if (nd === REVERSE[inward]) continue;
          const total = gn + (nd === inward ? 0 : BEND);
          if (total < best) {
            best = total;
            bestState = ns;
          }
          continue;
        }
        heap.push(gn + hEst(u), ns);
      }
    }
    if (bestState < 0) return [start, goal];
    const path: number[] = [];
    for (let s = bestState; s >= 0; s = came[s]) path.push(s >> 2);
    return path.reverse();
  };

  const pointsOf = (w: WireIn, path: number[]): Pt[] => {
    const [la, lb] = stubLen[w.id];
    const all = [{ x: w.from.x, y: w.from.y }, stubPoint(w.from, la), ...path.map(ptOf), stubPoint(w.to, lb), { x: w.to.x, y: w.to.y }];
    return simplify(all.map((p) => ({ x: r2(p.x), y: r2(p.y) })));
  };

  const order = [...wires].sort((a, b) => {
    const la = Math.abs(a.from.x - a.to.x) + Math.abs(a.from.y - a.to.y);
    const lb = Math.abs(b.from.x - b.to.x) + Math.abs(b.from.y - b.to.y);
    return la - lb || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  });
  occupyAll(null);
  for (const w of order) {
    const p = search(w);
    paths.set(w.id, p);
    occupy(p, netOf(w));
  }
  // once more, each wire with every other one in place
  for (const w of order) {
    occupyAll(w.id);
    paths.set(w.id, search(w));
  }
  for (const w of wires) out[w.id] = pointsOf(w, paths.get(w.id)!);
  return out;
}

/** The path of a routed wire with its corners rounded (radius `r`, less where
 *  a stretch is short). */
export function roundedPath(pts: Pt[], r = 6): string {
  if (pts.length === 0) return "";
  let d = `M${pts[0].x},${pts[0].y}`;
  for (let k = 1; k < pts.length - 1; k++) {
    const a = pts[k - 1];
    const p = pts[k];
    const b = pts[k + 1];
    const la = Math.hypot(p.x - a.x, p.y - a.y);
    const lb = Math.hypot(b.x - p.x, b.y - p.y);
    const rr = Math.min(r, la / 2, lb / 2);
    if (rr <= 0) {
      d += ` L${p.x},${p.y}`;
      continue;
    }
    const p1 = { x: p.x + ((a.x - p.x) / la) * rr, y: p.y + ((a.y - p.y) / la) * rr };
    const p2 = { x: p.x + ((b.x - p.x) / lb) * rr, y: p.y + ((b.y - p.y) / lb) * rr };
    d += ` L${p1.x},${p1.y} Q${p.x},${p.y} ${p2.x},${p2.y}`;
  }
  const last = pts[pts.length - 1];
  return `${d} L${last.x},${last.y}`;
}

/** The point halfway along a wire, for a mark on it. */
export function pathMiddle(pts: Pt[]): Pt {
  let total = 0;
  for (let k = 1; k < pts.length; k++) total += Math.abs(pts[k].x - pts[k - 1].x) + Math.abs(pts[k].y - pts[k - 1].y);
  let left = total / 2;
  for (let k = 1; k < pts.length; k++) {
    const a = pts[k - 1];
    const b = pts[k];
    const l = Math.abs(b.x - a.x) + Math.abs(b.y - a.y);
    if (l >= left && l > 0) return { x: a.x + ((b.x - a.x) * left) / l, y: a.y + ((b.y - a.y) * left) / l };
    left -= l;
  }
  return pts[0] ?? { x: 0, y: 0 };
}

/** The routed points with their ends on the wire's real ends on the screen,
 *  when these are where the route was made for (a port's dot sits a few units
 *  out from the outline); null when a part or port has moved since. */
export function fitEnds(pts: Pt[], s: Pt, t: Pt): Pt[] | null {
  if (pts.length < 2) return null;
  const out = pts.map((p) => ({ ...p }));
  const fit = (end: number, next: number, to: Pt) => {
    const e = out[end];
    const n = out[next];
    const alongX = e.y === n.y; // the stub runs along x
    const across = alongX ? Math.abs(e.y - to.y) : Math.abs(e.x - to.x);
    const along = alongX ? Math.abs(e.x - to.x) : Math.abs(e.y - to.y);
    if (across > 1.5 || along > 8) return false;
    out[end] = { ...to };
    if (alongX) out[next] = { x: n.x, y: to.y };
    else out[next] = { x: to.x, y: n.y };
    return true;
  };
  if (!fit(0, 1, s) || !fit(out.length - 1, out.length - 2, t)) return null;
  return out;
}
