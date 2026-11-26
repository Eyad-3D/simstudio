// The Energy view's figures under the part names: a figure that would cover
// a part, a part's name, the run's value over a part or another figure gives
// way (it is hidden) and the name stays. The figures of the parts that lose
// the most are placed first. Worked out on the drawn diagram, so it holds at
// every zoom: the names and figures keep their size on screen.

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

const overlaps = (a: Box, b: Box) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

/** For each figure (in the order given), whether it gives way. */
export function giveWay(figures: Box[], obstacles: Box[]): boolean[] {
  const kept: Box[] = [];
  return figures.map((f) => {
    if (obstacles.some((o) => overlaps(f, o)) || kept.some((k) => overlaps(f, k))) return true;
    kept.push(f);
    return false;
  });
}

const boxOf = (r: DOMRect): Box => ({ x: r.left, y: r.top, w: r.width, h: r.height });

/** The figure's own extent: its lines of text, not the block they sit in. */
function figureBox(el: Element): Box {
  const rs = [...el.children].map((c) => c.getBoundingClientRect()).filter((r) => r.width > 0);
  if (rs.length === 0) return boxOf(el.getBoundingClientRect());
  const x = Math.min(...rs.map((r) => r.left));
  const y = Math.min(...rs.map((r) => r.top));
  return { x, y, w: Math.max(...rs.map((r) => r.right)) - x, h: Math.max(...rs.map((r) => r.bottom)) - y };
}

/** Hide the figures in `root` that have no room; show the others. */
export function clearEnergyFigures(root: HTMLElement) {
  const figures = [...root.querySelectorAll<HTMLElement>("[data-energy-label]")];
  if (figures.length === 0) return;
  for (const f of figures) f.style.visibility = "";
  const obstacles = [...root.querySelectorAll("[data-node-box], [data-node-name], [data-live-badge]")].map((e) =>
    boxOf(e.getBoundingClientRect()),
  );
  const order = figures
    .map((el) => ({ el, lost: Number(el.dataset.lost ?? 0), box: figureBox(el) }))
    .sort((a, b) => b.lost - a.lost);
  const hide = giveWay(
    order.map((o) => o.box),
    obstacles,
  );
  order.forEach((o, i) => {
    o.el.style.visibility = hide[i] ? "hidden" : "";
    o.el.toggleAttribute("data-gave-way", hide[i]);
  });
}
