// Where the run's finish notice goes: its usual place, at the right just above
// the status bar, when it covers nothing a user clicks there; else the nearest
// place that covers nothing, up first (it keeps to the right edge), then left.
// It covered the bottom panel's "Collapse to tabs" button: a click on the
// button landed on the notice's tick box.

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

const STEP_UP = 8;
const STEP_LEFT = 16;
/** Room kept round a control: a notice that touches a button reads as on it. */
const ROOM = 3;

const hits = (a: Box, b: Box) =>
  a.x < b.x + b.w + ROOM && b.x - ROOM < a.x + a.w && a.y < b.y + b.h + ROOM && b.y - ROOM < a.y + a.h;

/** The notice's top-left: `home` when it is clear of every control, else the
 *  nearest clear place inside `view` (a step left counts as two up). */
export function placeNotice(size: { w: number; h: number }, home: { x: number; y: number }, controls: Box[], view: { w: number; h: number }): { x: number; y: number } {
  const ups = Math.max(0, Math.floor(home.y / STEP_UP));
  const lefts = Math.max(0, Math.floor(home.x / STEP_LEFT));
  const tries: [number, number][] = [];
  for (let u = 0; u <= ups; u++) for (let l = 0; l <= lefts; l++) tries.push([u, l]);
  tries.sort((a, b) => a[0] * STEP_UP + 2 * a[1] * STEP_LEFT - (b[0] * STEP_UP + 2 * b[1] * STEP_LEFT));
  for (const [u, l] of tries) {
    const at = { x: home.x - l * STEP_LEFT, y: home.y - u * STEP_UP };
    if (at.x < 0 || at.y < 0 || at.x + size.w > view.w) continue;
    const box = { ...at, w: size.w, h: size.h };
    if (!controls.some((c) => hits(box, c))) return at;
  }
  return home;
}

/**
 * The notice's `left` and `top` in its own CSS px, for a place `at` worked
 * out in the window's px. The notice sits in the status bar, which takes the
 * interface size as a CSS zoom (.ss-zoom): its left and top are drawn
 * multiplied by that `zoom`, while the window and getBoundingClientRect are
 * in the window's px. Unscaled, a notice placed at 125 % landed 25 % further
 * right and down, outside the window. Also kept wholly inside the window.
 */
export function noticeStyle(at: { x: number; y: number }, size: { w: number; h: number }, view: { w: number; h: number }, zoom: number) {
  const x = Math.max(0, Math.min(at.x, view.w - size.w));
  const y = Math.max(0, Math.min(at.y, view.h - size.h));
  const z = zoom > 0 ? zoom : 1;
  return { left: x / z, top: y / z };
}

/** The CSS zoom an element is drawn at: the product of its own and its
 *  ancestors' (Element.currentCSSZoom where the browser has it). */
export function cssZoomOf(el: Element): number {
  const own = (el as Element & { currentCSSZoom?: number }).currentCSSZoom;
  if (typeof own === "number" && own > 0) return own;
  let z = 1;
  for (let e: Element | null = el; e; e = e.parentElement) {
    const v = parseFloat(getComputedStyle(e).zoom);
    if (v > 0) z *= v;
  }
  return z;
}

/** What a user clicks on the page, outside `except` (the notice itself). */
export function clickables(except: Element): Box[] {
  const sel =
    "button, input, select, textarea, a[href], label, summary, [role=tab], [role=button], [role=menuitem], .react-flow__node, .react-flow__handle";
  const out: Box[] = [];
  for (const el of document.querySelectorAll(sel)) {
    if (except.contains(el)) continue;
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0 || r.bottom < 0 || r.top > innerHeight) continue;
    out.push({ x: r.left, y: r.top, w: r.width, h: r.height });
  }
  return out;
}
