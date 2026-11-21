// The arithmetic of the table tools (SpreadsheetGrid's right-click menus):
// the shading of cells by value, and filling, scaling, offsetting and
// interpolating a selection. Every number here is in the unit the cells show;
// the editors read the resulting text back as they read a typed value.

/** A number written without float noise (12 significant digits, as the
 *  import's tidy() and units.toStored keep). */
export const tidy = (n: number): number => Number(n.toPrecision(12));

// viridis (matplotlib's, by van der Walt and Smith), as [r, g, b] at ten
// even steps: perceptually uniform, and readable with the common
// colour-vision deficiencies. A scale for data, not a theme colour: the
// same in both themes, laid thinly over each theme's own background.
const VIRIDIS = [
  [68, 1, 84], [72, 40, 120], [62, 73, 137], [49, 104, 142], [38, 130, 142],
  [31, 158, 137], [53, 183, 121], [110, 206, 88], [181, 222, 43], [253, 231, 37],
];

/** How strongly a shade is laid over the cell's own background: light
 *  enough that the numbers keep 4.5:1 in both themes (contrast.test.ts). */
export const SHADE_ALPHA = 0.2;

/** The viridis colour at `t` (0 the lowest, 1 the highest), as [r, g, b]. */
export function viridis(t: number): number[] {
  const x = Math.min(1, Math.max(0, t)) * (VIRIDIS.length - 1);
  const i = Math.min(VIRIDIS.length - 2, Math.floor(x));
  return VIRIDIS[i].map((a, k) => Math.round(a + (VIRIDIS[i + 1][k] - a) * (x - i)));
}

/** The tint of a value between `lo` and `hi`, laid over the cell's own
 *  background (a gradient keeps the background colour underneath), or none
 *  when every value is the same. */
export function shadeStyle(v: number, lo: number, hi: number): { backgroundImage: string } | undefined {
  if (!(hi > lo) || !Number.isFinite(v)) return undefined;
  const c = `rgb(${viridis((v - lo) / (hi - lo)).join(" ")} / ${SHADE_ALPHA})`;
  return { backgroundImage: `linear-gradient(${c}, ${c})` };
}

/** A block of cells: null where a cell is empty. */
export type Block = (number | null)[][];

export const scaleBlock = (b: Block, f: number): Block => b.map((row) => row.map((v) => (v === null ? null : tidy(v * f))));
export const offsetBlock = (b: Block, d: number): Block => b.map((row) => row.map((v) => (v === null ? null : tidy(v + d))));

/**
 * The cells between a block's edges, filled linearly in the breakpoints'
 * values (`ys` down the rows, `xs` along the columns, so uneven steps are
 * followed as the engine interpolates). One row or column (or a block two
 * wide): each line between its two ends. Three or more each way: the inside
 * from all four edges (a Coons patch: exact on every edge, linear along a
 * plane). The edges are never changed. A message when it cannot be done.
 */
export function interpolateBlock(b: Block, ys: number[], xs: number[]): Block | string {
  const R = b.length;
  const C = b[0]?.length ?? 0;
  const at = (arr: number[], i: number) => {
    const d = arr[arr.length - 1] - arr[0];
    return Number.isFinite(d) && d !== 0 ? (arr[i] - arr[0]) / d : i / (arr.length - 1);
  };
  const empty = "an edge of the selection has an empty cell: fill the edges first.";
  const out = b.map((row) => [...row]);
  if (R >= 3 && C >= 3) {
    const edge = [...b[0], ...b[R - 1], ...b.map((row) => row[0]), ...b.map((row) => row[C - 1])];
    if (edge.some((v) => v === null)) return empty;
    const e = b as number[][];
    for (let i = 1; i < R - 1; i++) {
      const v = at(ys, i);
      for (let j = 1; j < C - 1; j++) {
        const u = at(xs, j);
        out[i][j] = tidy(
          (1 - v) * e[0][j] + v * e[R - 1][j] + (1 - u) * e[i][0] + u * e[i][C - 1] -
            ((1 - u) * (1 - v) * e[0][0] + u * (1 - v) * e[0][C - 1] + (1 - u) * v * e[R - 1][0] + u * v * e[R - 1][C - 1]),
        );
      }
    }
    return out;
  }
  if (C >= 3) {
    if (b.some((row) => row[0] === null || row[C - 1] === null)) return empty;
    for (const row of out) for (let j = 1; j < C - 1; j++) row[j] = tidy(row[0]! + (row[C - 1]! - row[0]!) * at(xs, j));
    return out;
  }
  if (R >= 3) {
    if (b[0].some((v) => v === null) || b[R - 1].some((v) => v === null)) return empty;
    for (let i = 1; i < R - 1; i++)
      for (let j = 0; j < C; j++) out[i][j] = tidy(b[0][j]! + (b[R - 1][j]! - b[0][j]!) * at(ys, i));
    return out;
  }
  return "select at least three cells in a row or a column: the cells between the first and the last are filled.";
}

/**
 * A new breakpoint for an axis whose breakpoints are `keys` (in order),
 * before or after the one at `i`: midway between its neighbours, or past an
 * end at the last step (1 when the axis has one breakpoint).
 */
export function newBreakpoint(keys: string[], i: number, where: "before" | "after"): string {
  const k = keys.map(Number);
  const j = where === "before" ? i - 1 : i; // the new one goes between j and j + 1
  const v =
    j >= 0 && j + 1 < k.length
      ? (k[j] + k[j + 1]) / 2
      : j < 0
        ? k[0] - (k.length > 1 ? k[1] - k[0] : 1)
        : k[j] + (k.length > 1 ? k[j] - k[j - 1] : 1);
  return String(tidy(v));
}

/** The breakpoints a paste past the end of an axis adds, as typed texts:
 *  each one step on from the last (the table's own last step). */
export function continueKeys(texts: string[]): string[] {
  const out = [...texts];
  out.forEach((t, i) => {
    if (t.trim() !== "") return;
    // (the empty ones before this are filled already)
    const known = out.slice(0, i).map((s) => Number(s.trim())).filter(Number.isFinite);
    const last = known.length ? known[known.length - 1] : 0;
    const step = known.length > 1 ? last - known[known.length - 2] : 1;
    out[i] = String(tidy(last + (step || 1)));
  });
  return out;
}
