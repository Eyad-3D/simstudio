// Where a menu opened at a point goes, so that all of it is inside the area it
// is drawn in (the diagram, which ends where the bottom panel starts). Below
// and right of the point when it fits there; flipped above or left of it when
// it fits there instead; else moved to the area's edge; a menu taller than the
// area starts at its top and scrolls.

export interface Size {
  width: number;
  height: number;
}

export interface MenuPlace {
  left: number;
  top: number;
  /** set when the menu is taller than the area: it scrolls */
  maxHeight?: number;
}

const MARGIN = 4;

function along(at: number, size: number, room: number): { pos: number; max?: number } {
  if (size > room - 2 * MARGIN) return { pos: MARGIN, max: Math.max(0, room - 2 * MARGIN) };
  if (at + size <= room - MARGIN) return { pos: Math.max(MARGIN, at) };
  if (at - size >= MARGIN) return { pos: at - size };
  return { pos: Math.max(MARGIN, room - MARGIN - size) };
}

export function placeMenu(x: number, y: number, menu: Size, area: Size): MenuPlace {
  const h = along(y, menu.height, area.height);
  const w = along(x, menu.width, area.width);
  return { left: w.pos, top: h.pos, ...(h.max !== undefined ? { maxHeight: h.max } : {}) };
}

/** Place a menu element (absolutely positioned in `area`) opened at (x, y). */
export function fitMenu(el: HTMLElement, area: HTMLElement, x: number, y: number) {
  el.style.maxHeight = "";
  const p = placeMenu(x, y, { width: el.offsetWidth, height: el.offsetHeight }, { width: area.clientWidth, height: area.clientHeight });
  el.style.left = `${p.left}px`;
  el.style.top = `${p.top}px`;
  el.style.maxHeight = p.maxHeight !== undefined ? `${p.maxHeight}px` : "";
  el.style.overflowY = p.maxHeight !== undefined ? "auto" : "";
}
