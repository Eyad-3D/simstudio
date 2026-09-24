import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { componentIcon } from "../icons";
import { ElementForm } from "./panels/PropertiesPanel";

/** Modal parameter editor opened by double-clicking an element on the canvas. */
export function ParameterDialog() {
  const paramDialogId = useUIStore((s) => s.paramDialogId);
  const paramDialogKey = useUIStore((s) => s.paramDialogKey);
  const closeParamDialog = useUIStore((s) => s.closeParamDialog);
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);

  useEffect(() => {
    if (!paramDialogId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeParamDialog();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
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

  // opened from a table's button: show that table
  const body = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (paramDialogKey)
      body.current?.querySelector(`[data-param="${paramDialogKey}"]`)?.scrollIntoView({ block: "start" });
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
      <div className="flex max-h-[82vh] w-[440px] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl">
        <div className="flex items-center gap-2 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-3 py-2">
          <Icon size={16} className="text-[color:var(--ss-accent)]" />
          <span className="text-[13px] font-semibold">{element.label}</span>
          <span className="text-[11px] text-[color:var(--ss-text-dim)]">— {def.name}</span>
          <button
            className="ss-toolbtn ml-auto"
            title="Close (Esc)"
            onClick={closeParamDialog}
          >
            <X size={14} />
          </button>
        </div>
        <div ref={body} className="min-h-0 flex-1 overflow-y-auto">
          <ElementForm element={element} def={def} />
        </div>
      </div>
    </div>
  );
}
