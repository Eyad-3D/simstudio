import { useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Search } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { componentIcon } from "../../icons";
import { cycleText } from "./CyclePicker";
import { searchLibrary, searchTerms } from "../../componentSearch";
import type { ComponentDef } from "../../types";

export function ComponentsPanel() {
  const library = useProjectStore((s) => s.library);
  const cycles = useProjectStore((s) => s.cycles);
  const placingId = useUIStore((s) => s.placingComponentId);
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [announcement, setAnnouncement] = useState("");
  const listRef = useRef<HTMLDivElement>(null);

  const groups = useMemo(() => {
    const filtered = searchLibrary(library, query);
    const byCat = new Map<string, ComponentDef[]>();
    for (const c of filtered) {
      if (!byCat.has(c.category)) byCat.set(c.category, []);
      byCat.get(c.category)!.push(c);
    }
    return [...byCat.entries()];
  }, [library, query]);
  // a search also finds the bundled drive cycles, by name, id or region
  // ("drive cycle" lists them all); each adds a Driving Task that drives it
  const task = library.find((d) => d.id === "signal.driving_task");
  const cycleHits = useMemo(() => {
    const terms = searchTerms(query);
    return terms.length
      ? cycles.filter((c) => terms.every((t) => `${c.name} ${c.id} ${c.region} drive cycle`.toLowerCase().includes(t)))
      : [];
  }, [cycles, query]);

  // Enter / Space / double-click: add the part in the middle of the diagram.
  // A diagram behind another tab (Monitors) or a maximised group has no size:
  // bring it to the front and add the part once React Flow has measured it
  // again (its ResizeObserver reports after the next layout, so two frames).
  const insert = (def: ComponentDef, then?: () => void) => {
    const ui = useUIStore.getState();
    ui.setPlacingComponent(null);
    const announce = (label: string | null | undefined) => {
      setAnnouncement(label ? `Added ${label} to the diagram.` : "Show the Topology panel to add parts.");
      if (label) then?.();
    };
    const label = ui.insertComponent?.(def.id);
    if (label) return announce(label);
    ui.focusPanel("topology");
    requestAnimationFrame(() =>
      requestAnimationFrame(() => announce(useUIStore.getState().insertComponent?.(def.id))),
    );
  };

  // arrow keys move between the rows (and category headers) of the list
  const onListKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const items = [...(listRef.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    const i = items.indexOf(e.target as HTMLButtonElement);
    if (i < 0) return;
    e.preventDefault();
    items[e.key === "ArrowDown" ? Math.min(i + 1, items.length - 1) : Math.max(i - 1, 0)].focus();
  };

  return (
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar">
        <Search size={13} className="text-[color:var(--ss-text-dim)]" />
        <input
          className="ss-input flex-1"
          placeholder="Search components…"
          aria-label="Search components"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <div ref={listRef} className="min-h-0 flex-1 overflow-y-auto py-1" onKeyDown={onListKeyDown}>
        {groups.map(([category, defs]) => {
          const isCollapsed = collapsed.has(category) && !query;
          return (
            <div key={category}>
              <button
                className="ss-tree-row font-semibold"
                aria-expanded={!isCollapsed}
                onClick={() =>
                  setCollapsed((prev) => {
                    const next = new Set(prev);
                    if (next.has(category)) next.delete(category);
                    else next.add(category);
                    return next;
                  })
                }
              >
                {isCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
                {category}
                <span className="ml-auto pr-1 text-[10px] font-normal text-[color:var(--ss-text-dim)]">
                  {defs.length}
                </span>
              </button>
              {!isCollapsed &&
                defs.map((def) => {
                  const Icon = componentIcon(def.icon);
                  const placing = placingId === def.id;
                  return (
                    <button
                      key={def.id}
                      className={`ss-tree-row cursor-grab pl-6 active:cursor-grabbing${placing ? " selected" : ""}`}
                      draggable
                      data-component-id={def.id}
                      // not aria-pressed: Enter adds the part rather than
                      // arming it; the diagram's banner announces an armed part
                      data-placing={placing || undefined}
                      aria-label={`Add ${def.name}`}
                      title={`${def.description ?? def.name}\n\nDrag onto the diagram, double-click or press Enter to add it in the middle, or click it and then click where it goes.`}
                      onDragStart={(e) => {
                        useUIStore.getState().setPlacingComponent(null);
                        e.dataTransfer.setData("application/lightsim", def.id);
                        e.dataTransfer.effectAllowed = "copy";
                      }}
                      onClick={(e) => {
                        // detail 0: Enter / Space; 1: a click arms click-to-place;
                        // 2: the second click of a double-click (handled below)
                        if (e.detail === 0) insert(def);
                        else if (e.detail === 1)
                          useUIStore.getState().setPlacingComponent(placing ? null : def.id);
                      }}
                      onDoubleClick={() => insert(def)}
                    >
                      <Icon size={14} strokeWidth={1.6} className="shrink-0 text-[color:var(--ss-node-icon)]" />
                      <span className="truncate">{def.name}</span>
                      <span className="ml-auto pr-1 text-[9px] uppercase text-[color:var(--ss-text-dim)]">
                        {def.domain.slice(0, 4)}
                      </span>
                    </button>
                  );
                })}
            </div>
          );
        })}
        {task && cycleHits.length > 0 && (
          <div>
            {/* lined up with the category names above */}
            <div className="py-[3px] pl-[25px] text-[12px] font-semibold">Drive cycles</div>
            {cycleHits.map((c) => {
              const Icon = componentIcon(task.icon);
              return (
                <button
                  key={c.id}
                  className="ss-tree-row pl-6"
                  aria-label={`Add a Driving Task on ${cycleText(c)}`}
                  title={`Adds a Driving Task that drives ${cycleText(c)} (${c.region})`}
                  onClick={(e) => {
                    // one task per double-click: its second click adds nothing
                    if (e.detail > 1) return;
                    // the new task is selected once it is on the diagram
                    insert(task, () => {
                      const st = useProjectStore.getState();
                      if (st.selectedElementId) st.setDrivingCycle(st.selectedElementId, c.id);
                    });
                  }}
                >
                  <Icon size={14} strokeWidth={1.6} className="shrink-0 text-[color:var(--ss-node-icon)]" />
                  <span className="truncate">{cycleText(c)}</span>
                </button>
              );
            })}
          </div>
        )}
        {groups.length === 0 && cycleHits.length === 0 && (
          <div className="px-3 py-2 text-[12px] text-[color:var(--ss-text-dim)]">
            No components match “{query}”.
          </div>
        )}
      </div>
      <div className="border-t border-[color:var(--ss-border)] px-2 py-1 text-[10px] text-[color:var(--ss-text-dim)]">
        Drag, double-click or press Enter to add; or click a part, then the diagram.
      </div>
      <div className="sr-only" aria-live="polite">
        {announcement}
      </div>
    </div>
  );
}
