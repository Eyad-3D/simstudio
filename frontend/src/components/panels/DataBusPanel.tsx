import { useId, useMemo, useState } from "react";
import { Trash2 } from "lucide-react";
import { countOf, portsOf, useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import type { PortDef } from "../../types";

/** A signal port of a part, as the panel lists it. */
interface End {
  elementId: string;
  port: PortDef;
  name: string; // "Vehicle · Vehicle Speed"
  unit: string;
}

/** A row: an input and the output feeding it (null: not connected), or a
 *  link between two inputs or two outputs, which passes no data (`odd`: only
 *  projects made before 0.3 have them). */
interface Row {
  to: End;
  from: End | null;
  linkId: string | null;
  odd?: boolean;
  /** the input's own key, so its box keeps focus when a source is picked */
  key: string;
}

/** Type-ahead choice of the output that feeds `to` (a WAI-ARIA combobox).
 *  Outputs with the input's unit come first; every typed word must match. */
function SourcePicker({
  to,
  from,
  outputs,
  onPick,
}: {
  to: End;
  from: End | null;
  outputs: End[];
  onPick: (from: End) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();
  // built only while the list is open: a big model has hundreds of inputs
  const matches = useMemo(() => {
    if (!open) return [];
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const unit = to.port.unitGroup && to.port.unitGroup !== "No Unit" ? to.port.unitGroup : null;
    return outputs
      .filter((o) => words.every((w) => `${o.name} ${o.unit}`.toLowerCase().includes(w)))
      .sort((a, b) => (unit ? Number(b.port.unitGroup === unit) - Number(a.port.unitGroup === unit) : 0));
  }, [open, outputs, query, to]);
  const choose = (o: End | undefined) => {
    if (o) onPick(o);
    setOpen(false);
    setQuery("");
  };
  // The list opens under the box, in the room left in the panel; in a short
  // tray the box first moves up to the top of the panel.
  const [room, setRoom] = useState(160);
  const openAt = (input: HTMLInputElement) => {
    if (open) return;
    const panel = input.closest("ul");
    if (panel) {
      const z = input.getBoundingClientRect().height / input.offsetHeight || 1; // the UI scale
      const below = () => (panel.getBoundingClientRect().bottom - input.getBoundingClientRect().bottom) / z;
      if (below() < 100) panel.scrollTop += (input.getBoundingClientRect().top - panel.getBoundingClientRect().top) / z;
      setRoom(Math.max(44, Math.min(160, below() - 4)));
    }
    setActive(0);
    setOpen(true);
  };
  return (
    <div className="min-w-0 flex-1">
      <input
        role="combobox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-autocomplete="list"
        aria-activedescendant={open && matches[active] ? `${listId}-${active}` : undefined}
        aria-label={`Source of ${to.name}`}
        className="ss-input w-full"
        placeholder="Pick a source…"
        value={open ? query : (from?.name ?? "")}
        onClick={(e) => openAt(e.currentTarget)}
        onBlur={() => setOpen(false)}
        onChange={(e) => {
          setQuery(e.target.value);
          setActive(0);
          openAt(e.currentTarget);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" && !open) openAt(e.currentTarget);
          else if (e.key === "ArrowDown") setActive((a) => Math.min(a + 1, matches.length - 1));
          else if (e.key === "ArrowUp") setActive((a) => Math.max(a - 1, 0));
          else if (e.key === "Enter" && open) choose(matches[active]);
          else if (e.key === "Escape" && open) setOpen(false);
          else return;
          e.preventDefault();
        }}
      />
      {open && (
        <ul
          id={listId}
          role="listbox"
          aria-label={`Outputs for ${to.name}`}
          style={{ maxHeight: room }}
          className="overflow-y-auto border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)]"
        >
          {matches.map((o, i) => (
            <li
              key={`${o.elementId}:${o.port.id}`}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === active}
              ref={i === active ? (el) => el?.scrollIntoView({ block: "nearest" }) : undefined}
              className={`cursor-pointer px-2 py-[2px] text-[11px] ${i === active ? "bg-[color:var(--ss-accent-soft)]" : ""}`}
              onMouseDown={(e) => e.preventDefault()} // keep focus, so the pick lands before blur closes the list
              onMouseEnter={() => setActive(i)}
              onClick={() => choose(o)}
            >
              {o.name} <span className="text-[color:var(--ss-text-dim)]">[{o.unit}]</span>
            </li>
          ))}
          {matches.length === 0 && (
            <li className="px-2 py-[2px] text-[11px] text-[color:var(--ss-text-dim)]">No output matches.</li>
          )}
        </ul>
      )}
    </div>
  );
}

/** Data Bus Connections: one row per signal input, with the output that
 *  feeds it picked from a type-ahead list. */
export function DataBusPanel() {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const unitGroups = useProjectStore((s) => s.unitGroups);
  const selectedId = useProjectStore((s) => s.selectedElementId);
  const addDataBus = useProjectStore((s) => s.addDataBus);
  const removeDataBus = useProjectStore((s) => s.removeDataBus);
  const selectedOnly = useUIStore((s) => s.busSelectedOnly);
  const setSelectedOnly = useUIStore((s) => s.setBusSelectedOnly);
  const [query, setQuery] = useState("");
  const [freeOnly, setFreeOnly] = useState(false);

  const { rows, outputs, portless } = useMemo(() => {
    const elements = project?.systems.flatMap((s) => s.elements) ?? [];
    const ends = new Map<string, End>();
    for (const el of elements) {
      for (const port of portsOf(el, libraryById)) {
        if (port.kind !== "signal") continue;
        const unit = unitGroups[port.unitGroup ?? "No Unit"] ?? "-";
        ends.set(`${el.id}:${port.id}`, { elementId: el.id, port, name: `${el.label} · ${port.name}`, unit });
      }
    }
    const linked = new Set<string>();
    const rows: Row[] = [];
    for (const d of project?.dataBusConnections ?? []) {
      const a = ends.get(`${d.element1Id}:${d.port1Id}`);
      const b = ends.get(`${d.element2Id}:${d.port2Id}`);
      if (!a || !b) continue;
      const [from, to] = a.port.direction === "output" ? [a, b] : [b, a];
      const input = `${to.elementId}:${to.port.id}`;
      const odd = from.port.direction === to.port.direction;
      // (an input with two sources, made before 0.3, lists both)
      rows.push({ from, to, linkId: d.id, odd, key: odd || linked.has(input) ? d.id : input });
      if (!odd) linked.add(input);
    }
    for (const [key, e] of ends) {
      if (e.port.direction === "input" && !linked.has(key)) rows.push({ from: null, to: e, linkId: null, key });
    }
    rows.sort((x, y) => x.to.name.localeCompare(y.to.name));
    const outputs = [...ends.values()]
      .filter((e) => e.port.direction === "output")
      .sort((x, y) => x.name.localeCompare(y.name));
    // a Monitor or Script has no ports until they are added in Properties
    const portless = elements.filter(
      (el) => libraryById[el.componentDefId]?.allowDynamicPorts && !el.dynamicPorts?.length,
    );
    return { rows, outputs, portless };
  }, [project, libraryById, unitGroups]);

  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const hit = (text: string) => words.every((w) => text.toLowerCase().includes(w));
  const mine = (...ids: (string | undefined)[]) => !selectedOnly || ids.includes(selectedId ?? "");
  const shown = rows.filter(
    (r) => (!freeOnly || !r.from) && mine(r.to.elementId, r.from?.elementId) && hit(`${r.from?.name ?? ""} ${r.to.name}`),
  );
  const hints = portless.filter((el) => mine(el.id) && hit(el.label));
  const free = rows.filter((r) => !r.from).length;

  return (
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar gap-2">
        <input
          className="ss-input w-[200px]"
          placeholder="Search signals…"
          aria-label="Search signals"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <label className="flex items-center gap-1 text-[11px]">
          <input type="checkbox" checked={freeOnly} onChange={(e) => setFreeOnly(e.target.checked)} />
          Unconnected inputs
        </label>
        <label className="flex items-center gap-1 text-[11px]">
          <input type="checkbox" checked={selectedOnly} onChange={(e) => setSelectedOnly(e.target.checked)} />
          Selected part
        </label>
        <span className="ml-auto text-[11px] text-[color:var(--ss-text-dim)]">
          {countOf(project?.dataBusConnections.length ?? 0, "link")} · {countOf(free, "unconnected input")}
        </span>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto">
        {shown.map((r) => (
          <li
            key={r.key}
            className="flex items-start gap-1 border-b border-[color:var(--ss-td-border)] px-2 py-[3px] text-[11px]"
          >
            {r.odd ? (
              <span className="min-w-0 flex-1 text-[color:var(--ss-warn)]">
                {r.from?.name} ↔ {r.to.name}: both {r.to.port.direction}s, so no data flows. Remove it.
              </span>
            ) : (
              <>
                <SourcePicker
                  to={r.to}
                  from={r.from}
                  outputs={outputs}
                  onPick={(o) => addDataBus(o.elementId, o.port.id, r.to.elementId, r.to.port.id, r.linkId ?? undefined)}
                />
                <span className="mt-[3px] shrink-0 text-[color:var(--ss-text-dim)]" aria-hidden>
                  →
                </span>
                <span className="mt-[3px] w-[45%] shrink-0 truncate" title={`${r.to.name} [${r.to.unit}]`}>
                  <b>{r.to.name}</b> <span className="text-[color:var(--ss-text-dim)]">[{r.to.unit}]</span>
                </span>
              </>
            )}
            {r.linkId && (
              <button className="ss-toolbtn shrink-0" title="Remove connection" onClick={() => removeDataBus(r.linkId!)}>
                <Trash2 size={12} />
              </button>
            )}
          </li>
        ))}
        {hints.map((el) => (
          <li key={el.id} className="px-2 py-[3px] text-[11px] text-[color:var(--ss-text-dim)]">
            <b>{el.label}</b>: no ports yet. Add one in Properties.
          </li>
        ))}
        {shown.length === 0 && hints.length === 0 && (
          <li className="px-3 py-2 text-[11px] text-[color:var(--ss-text-dim)]">
            {rows.length ? "No signal matches." : "No part has a signal input yet."}
          </li>
        )}
      </ul>
    </div>
  );
}
