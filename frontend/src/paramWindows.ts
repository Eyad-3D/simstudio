// A part's parameters in a window of their own: a double-click on a part
// opens a real window (a pop-up in a browser, a window of the desktop app)
// that the user can resize, minimise and move to another screen, one per
// part. The window is a blank page the main window draws into
// (components/ParamWindows.tsx renders the form there through a React
// portal), so it shares the main window's stores: an edit in it is in the
// project at once, and any change to the project shows in it at once,
// with nothing to send between windows.
import { create } from "zustand";
import { onAppKey } from "./appKeys";
import { PARAM_WINDOW_KEY } from "./storageKeys";
import { useUIStore } from "./store/uiStore";

export interface ParamWindow {
  /** the part's element id */
  id: string;
  win: Window;
  /** where the form is drawn, in `win`'s document */
  root: HTMLElement;
  /** the table to show (a narrow panel's table button), and a count that
   *  changes on every request for one, so the same table scrolls in again */
  paramKey: string | null;
  seq: number;
  /** false until the window has the main window's stylesheets: the form is
   *  drawn only then. Drawn sooner it is laid out unstyled, and loses its
   *  place (the table it was opened at) when the styles arrive. */
  ready: boolean;
}

/** The parameter windows open now, in the order they were opened. */
export const useParamWindows = create<{ windows: ParamWindow[] }>(() => ({ windows: [] }));

/** The window name a part's window is opened under. The desktop shell lets
 *  a page open a blank window only under a name with this prefix
 *  (desktop/src/windows.js). */
export const WINDOW_NAME_PREFIX = "lightsim-param-";

/** One remembered size and place: the last window closed. */
interface Geometry {
  left: number;
  top: number;
  width: number;
  height: number;
}
const DEFAULT_SIZE = { width: 460, height: 640 };
/** each further open window opens this far down and right of the last */
const CASCADE = 28;

function loadGeometry(): Geometry | null {
  try {
    const g = JSON.parse(window.localStorage.getItem(PARAM_WINDOW_KEY) ?? "null") as Geometry | null;
    if (g && [g.left, g.top, g.width, g.height].every(Number.isFinite) && g.width > 0 && g.height > 0) return g;
  } catch {
    /* storage unavailable, or not ours */
  }
  return null;
}

function saveGeometry(win: Window): void {
  // a window already gone (or never shown) has no size to keep
  if (!(win.innerWidth > 0 && win.innerHeight > 0)) return;
  const g: Geometry = { left: win.screenX, top: win.screenY, width: win.innerWidth, height: win.innerHeight };
  try {
    window.localStorage.setItem(PARAM_WINDOW_KEY, JSON.stringify(g));
  } catch {
    /* storage unavailable: this session only */
  }
}

/** The window's features: the remembered size and place, else a window the
 *  old dialog's size centred over the main window. The browser keeps a
 *  pop-up on a screen it has; the desktop shell moves one whose screen has
 *  gone back onto a screen (desktop/src/windows.js). */
function features(open: number): string {
  const g = loadGeometry() ?? {
    ...DEFAULT_SIZE,
    left: Math.round(window.screenX + (window.outerWidth - DEFAULT_SIZE.width) / 2),
    top: Math.round(window.screenY + (window.outerHeight - DEFAULT_SIZE.height) / 3),
  };
  const shift = open * CASCADE;
  return `popup,width=${Math.round(g.width)},height=${Math.round(g.height)},left=${Math.round(g.left + shift)},top=${Math.round(g.top + shift)}`;
}

/** The main window's look in `doc`: its stylesheets, theme and UI scale,
 *  now and whenever they change (the theme switch, the font size). */
function mirrorLook(doc: Document): MutationObserver {
  for (const node of document.head.querySelectorAll('style, link[rel="stylesheet"]')) {
    doc.head.appendChild(doc.importNode(node, true));
  }
  const from = document.documentElement;
  const to = doc.documentElement;
  const copy = () => {
    to.className = from.className;
    to.style.cssText = from.style.cssText;
  };
  copy();
  const observer = new MutationObserver(copy);
  observer.observe(from, { attributes: true, attributeFilter: ["class", "style"] });
  return observer;
}

/** `doc`'s copied stylesheets that are still loading: a built page's
 *  stylesheet is a file, fetched again for the new window. */
function loadingSheets(doc: Document): HTMLLinkElement[] {
  return [...doc.head.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]')].filter((l) => !l.sheet);
}

const observers = new WeakMap<Window, MutationObserver>();

function forget(id: string): void {
  const { windows } = useParamWindows.getState();
  const entry = windows.find((w) => w.id === id);
  if (!entry) return;
  observers.get(entry.win)?.disconnect();
  useParamWindows.setState({ windows: windows.filter((w) => w !== entry) });
}

/** Bring a window to the front, from its taskbar button if minimised. The
 *  desktop shell does it (a page cannot raise another window there); a
 *  browser raises a pop-up its opener focuses. */
function raise(entry: ParamWindow): void {
  if (window.lightsimDesktop?.raiseWindow) void window.lightsimDesktop.raiseWindow(entry.win.name);
  else entry.win.focus();
}

/** Open part `id`'s parameters in their window, or bring its open window to
 *  the front (at table `paramKey`, if given). False when the browser blocked
 *  the pop-up. */
export function openParamWindow(id: string, paramKey?: string): boolean {
  const { windows } = useParamWindows.getState();
  const open = windows.find((w) => w.id === id);
  if (open && !open.win.closed) {
    if (paramKey) {
      useParamWindows.setState({
        windows: windows.map((w) => (w === open ? { ...w, paramKey, seq: w.seq + 1 } : w)),
      });
    }
    raise(open);
    return true;
  }
  if (open) forget(id); // closed without telling us
  const name = `${WINDOW_NAME_PREFIX}${id}`;
  const win = window.open("", name, features(useParamWindows.getState().windows.length));
  if (!win) return false;
  // a blank page in standards mode (a new blank window is in quirks mode,
  // where tables do not take their font from the page)
  const doc = win.document;
  doc.open();
  doc.write('<!doctype html><html><head><meta charset="utf-8"></head><body><div id="root"></div></body></html>');
  doc.close();
  doc.documentElement.lang = document.documentElement.lang || "en";
  observers.set(win, mirrorLook(doc));
  // (listeners after doc.open(), which removes the window's listeners)
  win.addEventListener("keydown", onAppKey);
  // the user closed it (or it is going away with the main window)
  win.addEventListener("pagehide", () => {
    saveGeometry(win);
    forget(id);
  });
  const root = doc.getElementById("root")!;
  const loading = loadingSheets(doc);
  useParamWindows.setState((s) => ({
    windows: [...s.windows, { id, win, root, paramKey: paramKey ?? null, seq: 0, ready: loading.length === 0 }],
  }));
  // (a stylesheet that fails to load must not leave the window blank)
  void Promise.all(
    loading.map(
      (link) =>
        new Promise((done) => {
          link.addEventListener("load", done, { once: true });
          link.addEventListener("error", done, { once: true });
        }),
    ),
  ).then(() => {
    if (!loading.length) return;
    useParamWindows.setState((s) => ({ windows: s.windows.map((w) => (w.win === win ? { ...w, ready: true } : w)) }));
  });
  win.focus();
  return true;
}

/** Close part `id`'s window, if it has one. Nothing in the project changes. */
export function closeParamWindow(id: string): void {
  const entry = useParamWindows.getState().windows.find((w) => w.id === id);
  if (!entry) return;
  saveGeometry(entry.win);
  forget(id);
  entry.win.close();
}

export function closeAllParamWindows(): void {
  for (const w of useParamWindows.getState().windows) closeParamWindow(w.id);
}

/** Whether a parameter window is on screen (not minimised): the project's
 *  file is watched for an AI agent's edits while one is. */
export function paramWindowShown(): boolean {
  return useParamWindows.getState().windows.some((w) => !w.win.closed && w.win.document.visibilityState === "visible");
}

// the main window closing or reloading takes its windows with it: their
// content is drawn by this page and would be left blank
window.addEventListener("pagehide", closeAllParamWindows);

export function setWindowTitle(win: Window, title: string): void {
  win.document.title = title;
}

/** Show parameter `key` of a part's form (a Problems row or a table's button
 *  opened the window at it): in view (a table from its top, a value's row in
 *  the middle) and marked a moment so the eye finds it. */
export function showParam(form: HTMLElement | null, key: string): void {
  const el = form?.querySelector<HTMLElement>(`[data-param="${key}"]`);
  if (!el) return;
  // (in a folded section: unfold it)
  const section = el.closest("details");
  if (section) section.open = true;
  el.scrollIntoView({ block: el.tagName === "TR" ? "center" : "start" });
  mark(el);
}

/** The name the desktop shell knows the main window by, to raise it. */
export const MAIN_WINDOW_NAME = "lightsim-main";

/** Show the active case's own value of part `elementId`'s parameter `key` in
 *  Cases & Parameters (a parameter window's case line): the main window to
 *  the front, the panel shown, the value's row in view, focused and marked. */
export function showCaseValue(elementId: string, key: string): void {
  useUIStore.getState().focusPanel("cases");
  if (window.lightsimDesktop?.raiseWindow) void window.lightsimDesktop.raiseWindow(MAIN_WINDOW_NAME);
  else window.focus();
  requestAnimationFrame(() => {
    const row = document.querySelector<HTMLElement>(`[data-override="${CSS.escape(`${elementId}:${key}`)}"]`);
    if (!row) return;
    row.scrollIntoView({ block: "center" });
    row.querySelector<HTMLElement>("input, select")?.focus();
    mark(row);
  });
}

/** Marked a moment so the eye finds it. */
function mark(el: HTMLElement): void {
  el.classList.remove("ss-param-mark");
  void el.offsetWidth; // (so a second request marks it again)
  el.classList.add("ss-param-mark");
  el.addEventListener("animationend", () => el.classList.remove("ss-param-mark"), { once: true });
}
