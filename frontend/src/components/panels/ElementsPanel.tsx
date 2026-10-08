import { useActiveSystem, useProjectStore } from "../../store/projectStore";
import { componentIcon } from "../../icons";
import { deleteSelection, renameSelection } from "../../store/canvasSelection";

export function ElementsPanel() {
  const system = useActiveSystem();
  const libraryById = useProjectStore((s) => s.libraryById);
  const selectedElementId = useProjectStore((s) => s.selectedElementId);
  const select = useProjectStore((s) => s.select);
  const setActiveSystem = useProjectStore((s) => s.setActiveSystem);

  if (!system) return null;
  return (
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar text-[11px] text-[color:var(--ss-text-dim)]">
        Elements in <b className="ml-1 text-[color:var(--ss-text)]">{system.name}</b>
        <span className="ml-auto">{system.elements.length}</span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto py-1">
        {system.elements.map((el) => {
          const def = libraryById[el.componentDefId];
          const Icon = componentIcon(def?.icon ?? "box");
          return (
            <button
              key={el.id}
              className={`ss-tree-row ${selectedElementId === el.id ? "selected" : ""}`}
              onClick={() => select(el.id)}
              // the diagram's keys work on the part picked here too (UX-19)
              onKeyDown={(e) => {
                if (e.ctrlKey || e.metaKey || e.altKey) return;
                if (e.key === "Delete" || e.key === "Backspace") {
                  if (selectedElementId !== el.id) select(el.id);
                  if (deleteSelection()) e.preventDefault();
                } else if (e.key === "F2") {
                  if (selectedElementId !== el.id) select(el.id);
                  if (renameSelection()) e.preventDefault();
                }
              }}
              onDoubleClick={() => {
                if (el.isSubSystem && el.subSystemId) setActiveSystem(el.subSystemId);
              }}
              title={def?.name}
            >
              <Icon size={14} strokeWidth={1.6} className="shrink-0 text-[color:var(--ss-node-icon)]" />
              <span className="truncate">{el.label}</span>
              <span className="ml-auto pr-1 text-[10px] text-[color:var(--ss-text-dim)]">
                {def?.name}
              </span>
            </button>
          );
        })}
        {system.elements.length === 0 && (
          <div className="px-3 py-2 text-[12px] text-[color:var(--ss-text-dim)]">
            This system is empty.
          </div>
        )}
      </div>
    </div>
  );
}
