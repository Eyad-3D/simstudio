"use strict";

/**
 * The windows the page may open. A part's parameters open in a window of
 * their own (frontend/src/paramWindows.ts): a blank page opened under a name
 * that starts with PARAM_PREFIX, which the main window then draws into. The
 * app's help opens in one Help window (frontend/src/help.ts), named
 * HELP_NAME, from the main window or a parameter window, and only at the
 * help pages the engine serves (under /help/ on the app's own origin). Those
 * are the only windows the page gets; an http(s) link goes to the system
 * browser and anything else is refused, as before.
 */

const PARAM_PREFIX = "lightsim-param-";
const HELP_NAME = "lightsim-help";

/**
 * Whether `url` is one of the app's help pages: on `origin` (the engine's,
 * null before it runs), under /help/ (the URL parser has already resolved
 * any "..", also when written %2e%2e), with no user name or password in it.
 * @param {string} url
 * @param {string | null} origin
 */
function isHelpUrl(url, origin) {
  if (!origin) return false;
  let u;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  return u.origin === origin && !u.username && !u.password && u.pathname.startsWith("/help/");
}

/**
 * What to do with a window.open() from a page, from Electron's details:
 * "param" (allow a parameter window: only the main window opens one),
 * "help" (allow the Help window: from the main window or a parameter
 * window, at a help page), "external" (hand the link to the system browser)
 * or "deny". `origin` is the engine's; `from` the kind of window asking.
 * @param {{ url: string, frameName?: string }} details
 * @param {string | null} [origin]
 * @param {"main" | "param" | "help"} [from]
 */
function windowOpenAction({ url, frameName }, origin = null, from = "main") {
  const blank = url === "about:blank" || url === "";
  const named = typeof frameName === "string" && frameName.startsWith(PARAM_PREFIX) && frameName.length > PARAM_PREFIX.length;
  if (blank && named && from === "main") return "param";
  if (frameName === HELP_NAME && from !== "help" && isHelpUrl(url, origin)) return "help";
  if (/^https?:\/\//i.test(url)) return "external";
  return "deny";
}

/**
 * A parameter window's and the Help window's BrowserWindow options: the main window's safety (no
 * Node in the page, context isolation, the sandbox) and no preload, so no
 * shell calls of its own; the main window does all the talking. Its size
 * is the page's (the features string), as the content's size.
 * @param {string} icon
 */
function paramWindowOptions(icon) {
  return {
    icon,
    minWidth: 320,
    minHeight: 240,
    useContentSize: true,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      spellcheck: false,
    },
  };
}

/** How much of a window's title bar must be on a screen to be left alone. */
const GRAB = { width: 120, height: 24 };

/**
 * A window's bounds moved back onto a screen when its remembered place is on
 * a monitor that has gone (or off every screen): centred on `fallback` (the
 * main window's screen), no larger than it. Null when it can stay.
 * @param {{x: number, y: number, width: number, height: number}} bounds
 * @param {{x: number, y: number, width: number, height: number}[]} workAreas
 * @param {{x: number, y: number, width: number, height: number}} fallback
 */
function onScreen(bounds, workAreas, fallback) {
  const titleBar = { x: bounds.x, y: bounds.y, width: bounds.width, height: GRAB.height };
  const reachable = workAreas.some((a) => {
    const w = Math.min(titleBar.x + titleBar.width, a.x + a.width) - Math.max(titleBar.x, a.x);
    const h = Math.min(titleBar.y + titleBar.height, a.y + a.height) - Math.max(titleBar.y, a.y);
    return w >= Math.min(GRAB.width, bounds.width) && h >= GRAB.height;
  });
  if (reachable) return null;
  const width = Math.min(bounds.width, fallback.width);
  const height = Math.min(bounds.height, fallback.height);
  return {
    x: fallback.x + Math.round((fallback.width - width) / 2),
    y: fallback.y + Math.round((fallback.height - height) / 2),
    width,
    height,
  };
}

/** The app's panels by id and tab title, as the page's DockLayout lists them
 *  (PANELS in frontend/src/components/DockLayout.tsx; a test keeps the two
 *  the same). */
const PANELS = [
  ["components", "Components"],
  ["elements", "Elements"],
  ["topology", "Topology"],
  ["monitors", "Monitors"],
  ["properties", "Properties"],
  ["cases", "Cases"],
  ["messages", "Messages"],
  ["data-checks", "Problems"],
  ["layer-config", "Layer Configurations"],
  ["data-bus", "Data Bus Connections"],
  ["mini-chart", "Signal Plot"],
];

/**
 * The View menu: every panel by name (one that was closed comes back), the
 * layout and the theme, the zoom and full screen. Reload and the developer
 * tools only when the app runs from its sources (`dev`, not packaged): in the
 * released app Reload would throw away the page's unsaved state. `call` runs
 * a script in the main window's page.
 */
function viewMenu({ dev, call }) {
  const page = (script) => () => call(script);
  return [
    {
      label: "Panels",
      submenu: PANELS.map(([id, title]) => ({
        label: title,
        click: page(`window.lightsimShowPanel ? window.lightsimShowPanel(${JSON.stringify(id)}) : false`),
      })),
    },
    { label: "Reset Layout", click: page("window.lightsimResetLayout ? window.lightsimResetLayout() : false") },
    { type: "separator" },
    { label: "Light Theme", click: page('window.lightsimSetTheme ? window.lightsimSetTheme("light") : false') },
    { label: "Dark Theme", click: page('window.lightsimSetTheme ? window.lightsimSetTheme("dark") : false') },
    { type: "separator" },
    { role: "resetZoom" },
    { role: "zoomIn" },
    { role: "zoomOut" },
    { type: "separator" },
    { role: "togglefullscreen" },
    ...(dev ? [{ type: "separator" }, { role: "reload" }, { role: "forceReload" }, { role: "toggleDevTools" }] : []),
  ];
}

module.exports = { PARAM_PREFIX, HELP_NAME, PANELS, isHelpUrl, windowOpenAction, paramWindowOptions, onScreen, viewMenu };
