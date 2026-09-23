import { useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Search } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { componentIcon } from "../../icons";
import type { ComponentDef } from "../../types";

// Other names people search for. The library names follow its own vocabulary
// ("E-Motor", "HV Battery Pack"); these are matched like the descriptions.
const SYNONYMS: Record<string, string> = {
  "battery.generic": "accumulator energy storage ess cell",
  "motor.emotor": "inverter electric machine traction motor generator",
  "controller.dcdc": "dcdc converter",
  "engine.combustion": "ice internal combustion petrol gasoline diesel",
  "fuel.tank": "petrol gasoline diesel",
  "mech.gearbox": "transmission",
  "mech.final_drive": "axle reduction",
  "mech.transfer_case": "awd 4wd",
  "electric.constant_drive": "load auxiliaries hvac",
  "electric.node": "bus busbar junction",
  "boundary.ground": "earth",
  "propulsion.wheel": "tyre tire",
  "vehicle.body": "chassis car",
  "signal.driving_task": "drive cycle wltp nedc",
  "signal.road_profile": "slope gradient hill",
  "signal.script": "python code",
  "signal.monitor": "scope probe",
  "container.system": "subsystem group",
  "fuel.h2_tank": "h2",
};

/** Every search term must hit: anywhere in the name, category or id, or at
 *  the start of a word in the description or synonyms (so "ice" finds the
 *  engine but not "device"). */
function matches(def: ComponentDef, terms: string[]): boolean {
  const direct = `${def.name} ${def.category} ${def.id}`.toLowerCase();
  const words = `${def.description ?? ""} ${SYNONYMS[def.id] ?? ""}`.toLowerCase().split(/[^a-z0-9]+/);
  return terms.every((t) => direct.includes(t) || words.some((w) => w.startsWith(t)));
}

export function ComponentsPanel() {
  const library = useProjectStore((s) => s.library);
  const placingId = useUIStore((s) => s.placingComponentId);
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [announcement, setAnnouncement] = useState("");
  const listRef = useRef<HTMLDivElement>(null);

  const groups = useMemo(() => {
    const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const filtered = terms.length ? library.filter((c) => matches(c, terms)) : library;
    const byCat = new Map<string, ComponentDef[]>();
    for (const c of filtered) {
      if (!byCat.has(c.category)) byCat.set(c.category, []);
      byCat.get(c.category)!.push(c);
    }
    return [...byCat.entries()];
  }, [library, query]);

  // Enter / Space / double-click: add the part in the middle of the diagram.
  // A diagram behind another tab (Monitors) or a maximised group has no size:
  // bring it to the front and add the part once React Flow has measured it
  // again (its ResizeObserver reports after the next layout, so two frames).
  const insert = (def: ComponentDef) => {
    const ui = useUIStore.getState();
    ui.setPlacingComponent(null);
    const announce = (label: string | null | undefined) =>
      setAnnouncement(label ? `Added ${label} to the diagram.` : "Show the Topology panel to add parts.");
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
                        e.dataTransfer.setData("application/simstudio", def.id);
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
        {groups.length === 0 && (
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
