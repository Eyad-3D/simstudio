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
