import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { componentHelpPage, useHelpStore } from "../help";
import { componentIcon } from "../icons";
import { ElementForm } from "./panels/PropertiesPanel";
import { PartHelpButton } from "./ParamWindows";
import { showParam } from "../paramWindows";

/** A part's parameters in a modal dialog inside the main window: only when
 *  the browser blocked the parameter window (ParamWindows.tsx), which is
 *  how a double-click on a part opens them. */
export function ParameterDialog() {
  const paramDialogId = useUIStore((s) => s.paramDialogId);
  const paramDialogKey = useUIStore((s) => s.paramDialogKey);
  const closeParamDialog = useUIStore((s) => s.closeParamDialog);
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);

  useEffect(() => {
    if (!paramDialogId) return;
    const onKey = (e: KeyboardEvent) => {
      // the first Esc closes a parameter's help card (UX-10), the next the dialog
      // (an import preview over the dialog closes first, on its own)
      if (
        e.key === "Escape" &&
        !document.querySelector(".ss-help-card:popover-open") &&
        !document.querySelector(".ss-import-dialog")
      )
        closeParamDialog();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [paramDialogId, closeParamDialog]);

  // help shown in the Help panel (the browser refused the Help window, or
  // the desktop menu asked) would sit greyed out behind the dialog: the
  // dialog makes way. Its edits are already in the project.
  useEffect(() => {
    if (!paramDialogId) return;
    return useHelpStore.subscribe((s) => {
      if (s.page) closeParamDialog();
    });
  }, [paramDialogId, closeParamDialog]);

  const element = project?.systems
    .flatMap((s) => s.elements)
    .find((e) => e.id === paramDialogId);
  const def = element ? libraryById[element.componentDefId] : undefined;

  // the part went away under the dialog (undo, Delete, another project):
  // forget it too, or the menus would take the dialog for still open
  useEffect(() => {
    if (paramDialogId && !def) closeParamDialog();
  }, [paramDialogId, def, closeParamDialog]);

  // opened at a parameter (a table's button, a Problems row): show it
  const body = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (paramDialogKey) showParam(body.current, paramDialogKey);
  }, [paramDialogId, paramDialogKey]);

  if (!paramDialogId || !element || !def) return null;
  const Icon = componentIcon(def.icon);

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => {
        if (e.target !== e.currentTarget) return;
        // a click outside while a table cell is being edited only ends that
        // edit (the cell commits on blur); the next outside click closes
        if (document.activeElement?.classList.contains("ss-cell-input")) return;
        closeParamDialog();
      }}
    >
      <div
        className="flex max-h-[82vh] w-[440px] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
        data-help-part={componentHelpPage(def.id)}
      >
        <div className="flex items-center gap-2 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-3 py-2">
          {/* eslint-disable-next-line react-hooks/static-components -- componentIcon picks from a fixed module-level registry, so Icon is the same component every render */}
          <Icon size={16} className="text-[color:var(--ss-accent)]" />
          <span className="min-w-0 truncate text-title font-semibold">{element.label}</span>
          <span className="min-w-0 truncate text-meta text-[color:var(--ss-text-dim)]">— {def.name}</span>
          <PartHelpButton defId={def.id} from={window} />
          <button
            className="ss-toolbtn"
            title="Close (Esc)"
            onClick={closeParamDialog}
          >
            <X size={14} />
          </button>
        </div>
        <div ref={body} className="ss-param-scroll min-h-0 flex-1 overflow-y-auto">
          <ElementForm element={element} def={def} />
        </div>
      </div>
    </div>
  );
}
