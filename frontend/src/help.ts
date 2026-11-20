// The help pages (LRN-04) are static files the engine serves at /help/, built
// from the repo's Markdown by scripts/build-docs.mjs. They open in the Help
// panel inside the app (LRN-09, components/HelpPanel.tsx), which works with
// no network; the panel's "Open in browser" hands the page to the system
// browser for tabs and bookmarks.
import { useEffect, useState } from "react";
import { create } from "zustand";
import { LAST_VERSION_KEY } from "./storageKeys";

/** The help page of a library part, by its id ("signal.driving_task"). */
export const componentHelpPage = (defId: string) => `reference/components/${defId}.html`;

/** The help page of each dock panel, for its "?" and for F1 inside it. */
export const PANEL_HELP: Record<string, string> = {
  components: "reference/components/index.html",
  elements: "reference/components/index.html",
  topology: "tutorials/first-electric-car.html",
  monitors: "glossary.html#monitor",
  properties: "reference/components/index.html",
  cases: "how-to/parameter-sweep.html",
  messages: "how-to/fix-problems.html",
  "data-checks": "how-to/fix-problems.html",
  "layer-config": "glossary.html#layer-configurations",
  "data-bus": "how-to/wire-control-signals.html",
  "mini-chart": "glossary.html#channel",
  results: "reference/results.html",
};

/** The Help menu: in the header's Help button and the desktop app's menu. */
export const HELP_MENU: { label: string; page: string }[] = [
  { label: "Documentation", page: "index.html" },
  { label: "Your first electric car (tutorial)", page: "tutorials/first-electric-car.html" },
  { label: "Formula Student lessons", page: "lessons/fs-1-acceleration.html" },
  { label: "Examples guide", page: "examples/bev-car.html" },
  { label: "Results numbers explained", page: "reference/results.html" },
  { label: "Keyboard shortcuts", page: "reference/keyboard-shortcuts.html" },
  { label: "Glossary", page: "glossary.html" },
  { label: "Known limits", page: "known-limits.html" },
  { label: "What is validated", page: "validation.html" },
  { label: "Release notes", page: "release-notes.html" },
];

/** Where a user reports a problem. */
export const REPORT_PROBLEM_URL = "https://github.com/Eyad-3D/simstudio/issues";

/** The page the Help panel shows, or null when it is closed. */
export const useHelpStore = create<{ page: string | null }>(() => ({ page: null }));

export const helpUrl = (page = "index.html") => `${import.meta.env.BASE_URL}help/${page}`;

/** Open a help page in the Help panel inside the app. */
export function openHelp(page = "index.html"): void {
  useHelpStore.setState({ page });
}

export function closeHelp(): void {
  useHelpStore.setState({ page: null });
}

/** Open a help page in the system browser (the desktop app hands web links
 *  to it), for tabs and bookmarks. */
export function openHelpInBrowser(page = "index.html"): void {
  window.open(new URL(helpUrl(page), window.location.href).href, "_blank", "noopener");
}

/** The Help window's name: one window for the app's help, reused. The
 *  desktop shell lets a page open a window under this name only at the
 *  app's own help pages (desktop/src/windows.js). */
export const HELP_WINDOW_NAME = "lightsim-help";
let helpWindow: Window | null = null;

/** Open a help page in the Help window, a real window of its own beside a
 *  part's parameter window (help asked from there), or show it there and
 *  bring the window to the front if it is open. `from` is the window the
 *  user asked in: a browser lets only that window open a pop-up. Where the
 *  pop-up is refused, the page shows in the Help panel instead. */
export function openHelpWindow(page: string, from: Window = window): void {
  const url = new URL(helpUrl(page), window.location.href).href;
  if (helpWindow && !helpWindow.closed) {
    helpWindow.location.href = url;
  } else {
    helpWindow = from.open(url, HELP_WINDOW_NAME, "popup,width=820,height=860");
    if (!helpWindow) return openHelp(page);
  }
  if (window.lightsimDesktop?.raiseWindow) void window.lightsimDesktop.raiseWindow(HELP_WINDOW_NAME);
  else helpWindow.focus();
}

// the Help window belongs to the main window: it closes (or reloads) with it
window.addEventListener("pagehide", () => helpWindow?.close());

/** Show the release notes once after an update (What's new): `version` is
 *  the app's; a first launch only remembers it. Returns whether it opened. */
export function whatsNewOnce(version: string, storage: Storage = window.localStorage): boolean {
  let seen: string | null;
  try {
    seen = storage.getItem(LAST_VERSION_KEY);
    storage.setItem(LAST_VERSION_KEY, version);
  } catch {
    return false; // storage unavailable: never nag
  }
  if (seen === null || seen === version) return false;
  openHelp("release-notes.html");
  return true;
}

/** What a Results summary row means (LRN-10), from the help's Results
 *  reference (help/summary-terms.json, written by build-docs.mjs). */
export type SummaryTerm = { pattern: string; text: string };
let terms: Promise<{ re: RegExp; text: string }[]> | null = null;

export function summaryTerms(): Promise<{ re: RegExp; text: string }[]> {
  terms ??= fetch(helpUrl("summary-terms.json"))
    .then((r) => (r.ok ? (r.json() as Promise<SummaryTerm[]>) : []))
    .then((list) => list.map((t) => ({ re: new RegExp(t.pattern), text: t.text })))
    .catch(() => []);
  return terms;
}

/** The definition of each summary row label, once the terms have loaded. */
export function useSummaryDefinition(): (label: string) => string | undefined {
  const [list, setList] = useState<{ re: RegExp; text: string }[]>([]);
  useEffect(() => {
    let live = true;
    void summaryTerms().then((l) => live && setList(l));
    return () => {
      live = false;
    };
  }, []);
  return (label) => list.find((t) => t.re.test(label))?.text;
}

declare global {
  interface Window {
    /** For the desktop app's Help menu: open a help page in the panel. */
    lightsimHelp?: (page: string) => boolean;
  }
}
window.lightsimHelp = (page: string) => {
  openHelp(page);
  return true;
};
