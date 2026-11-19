import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { componentIcon } from "../icons";
import { OwnerWindow } from "../ownerWindow";
import {
  closeAllParamWindows,
  closeParamWindow,
  openParamWindow,
  setWindowTitle,
  useParamWindows,
  type ParamWindow,
} from "../paramWindows";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { ElementForm } from "./panels/PropertiesPanel";

/** Said in Messages once a session, the first time a pop-up is blocked. */
let blockedSaid = false;

/** The parts' parameter windows (paramWindows.ts): the form of each open
 *  window, drawn into it from here. A double-click on a part (and every
 *  other way to its parameters) asks the UI store, which asks this; a
 *  browser that blocks the pop-up gets the old dialog in the main window
 *  (ParameterDialog.tsx). */
export function ParamWindows() {
  const windows = useParamWindows((s) => s.windows);

  useEffect(() => {
    useUIStore.getState().setParamWindowOpener((id, paramKey) => {
      if (openParamWindow(id, paramKey)) return true;
      if (!blockedSaid) {
        blockedSaid = true;
        useProjectStore
          .getState()
          .log(
            "warning",
            "The browser blocked the parameter window, so parameters open in a dialog inside the main window instead. Allow pop-ups for this page to get a window per part.",
          );
      }
      return false;
    });
    // another project opened, or the project closed: its windows close (a
    // Save As gives the same project a new id, and keeps them)
    const unsub = useProjectStore.subscribe((s, prev) => {
      if (s.project === prev.project) return;
      if (!s.project || (s.project.id !== prev.project?.id && s.project.systems !== prev.project?.systems)) {
        closeAllParamWindows();
      }
    });
    return () => {
      unsub();
      useUIStore.getState().setParamWindowOpener(null);
    };
  }, []);

  return windows
    .filter((w) => w.ready)
    .map((w) =>
      createPortal(
        <OwnerWindow.Provider value={w.win}>
          <ParamWindowContent entry={w} />
        </OwnerWindow.Provider>,
        w.root,
        w.id,
      ),
    );
}

function ParamWindowContent({ entry }: { entry: ParamWindow }) {
  const element = useProjectStore((s) =>
    s.project?.systems.flatMap((x) => x.elements).find((e) => e.id === entry.id),
  );
  const def = useProjectStore((s) => (element ? s.libraryById[element.componentDefId] : undefined));

  // the part went away (Delete, an undo, an AI agent's edit): so does its window
  const gone = !element || !def;
  useEffect(() => {
    if (gone) closeParamWindow(entry.id);
  }, [gone, entry.id]);

  // the window is named after the part, and follows a rename
  const label = element?.label;
  useEffect(() => {
    if (label !== undefined) setWindowTitle(entry.win, label);
  }, [label, entry.win]);

  // the focus starts on the first field
  const body = useRef<HTMLDivElement>(null);
  useEffect(() => {
    body.current?.querySelector<HTMLElement>("input, select, textarea")?.focus();
  }, []);

  // opened from a table's button: show that table
  useEffect(() => {
    if (entry.paramKey)
      body.current?.querySelector(`[data-param="${entry.paramKey}"]`)?.scrollIntoView({ block: "start" });
  }, [entry.paramKey, entry.seq]);

  if (gone) return null;
  const Icon = componentIcon(def.icon);
  return (
    <div className="flex h-full flex-col bg-[color:var(--ss-panel)]">
      <header className="ss-zoom flex items-center gap-2 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-3 py-2">
        {/* eslint-disable-next-line react-hooks/static-components -- componentIcon picks from a fixed module-level registry, so Icon is the same component every render */}
        <Icon size={16} className="text-[color:var(--ss-accent)]" aria-hidden />
        <h1 className="text-[13px] font-semibold">{element.label}</h1>
        <span className="text-[11px] text-[color:var(--ss-text-dim)]">— {def.name}</span>
      </header>
      <main ref={body} className="min-h-0 flex-1 overflow-y-auto">
        <div className="ss-zoom">
          <ElementForm element={element} def={def} />
        </div>
      </main>
    </div>
  );
}
