import { useEffect, useId, useMemo, useRef, useState, type CSSProperties } from "react";
import { Trash2 } from "lucide-react";
import {
  applyBulkLinks,
  busIndex,
  inputGroups,
  keyOf,
  planConnectToAll,
  planMatchingNames,
  type BulkPlan,
} from "../../dataBusBulk";
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

/** A row: an input and the output feeding it (null: not connected). */
interface Row {
  to: End;
  from: End | null;
  linkId: string | null;
  /** the input's own key, so its box keeps focus when a source is picked */
  key: string;
}

/** A link that passes no data, listed so it can be removed: between two
 *  inputs or two outputs (only projects made before 0.3 have them), or to a
 *  part or port that is gone. */
interface Broken {
  id: string;
  text: string;
  elementIds: string[];
}

const wordsOf = (name: string) => name.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);

/** Type-ahead choice of the output that feeds `to` (a WAI-ARIA combobox).
 *  Outputs that share words with the input's name come first (Brake Command:
 *  Driver · Brake Command), then those with its unit; every typed word must
 *  match. */
function SourcePicker({
  to,
  from,
  outputs,
  onPick,
  label = `Source of ${to.name}`,
}: {
  to: End;
  from: End | null;
  outputs: End[];
  onPick: (from: End) => void;
  /** the box's name (default: "Source of <input>") */
  label?: string;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();
  const box = useRef<HTMLInputElement>(null);
  const boxTop = useRef(0); // where the box was when the list opened
  // built only while the list is open: a big model has hundreds of inputs
  const matches = useMemo(() => {
    if (!open) return [];
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const unit = to.port.unitGroup && to.port.unitGroup !== "No Unit" ? to.port.unitGroup : null;
    const own = wordsOf(to.port.name);
    const rank = (o: End) => {
      const theirs = new Set(wordsOf(o.name));
      return 2 * own.filter((w) => theirs.has(w)).length + Number(!!unit && o.port.unitGroup === unit);
    };
    return outputs
      .filter((o) => words.every((w) => `${o.name} ${o.unit}`.toLowerCase().includes(w)))
      .map((o) => ({ o, r: rank(o) }))
      .sort((a, b) => b.r - a.r) // stable: A to Z within a rank
      .map(({ o }) => o);
  }, [open, outputs, query, to]);
  const close = () => {
    setOpen(false);
    setQuery("");
  };
  const choose = (o: End | undefined) => {
    if (o) onPick(o);
    close();
  };
  // The list floats over the page under the box (over it when there is more
  // room above), so a short bottom panel still shows about ten outputs.
  const [place, setPlace] = useState<CSSProperties>({});
  const openAt = (input: HTMLInputElement) => {
    if (open) return;
    const r = input.getBoundingClientRect();
    boxTop.current = r.top;
    const z = input.currentCSSZoom || 1; // the UI scale
    const below = innerHeight - r.bottom;
    const up = below < 200 * z && r.top > below;
    setPlace({
      left: r.left / z,
      width: r.width / z,
      maxHeight: Math.min(200, (up ? r.top : below) / z - 4),
      ...(up ? { bottom: (innerHeight - r.top) / z } : { top: r.bottom / z }),
    });
    setActive(0);
    setOpen(true);
  };
  // placed once, so it closes when the box moves (the rows scroll)
  useEffect(() => {
    if (!open) return;
    const onScroll = () => {
      if (box.current?.getBoundingClientRect().top === boxTop.current) return;
      setOpen(false);
      setQuery("");
    };
    addEventListener("scroll", onScroll, true);
    return () => removeEventListener("scroll", onScroll, true);
  }, [open]);
  return (
    <div className="min-w-0 flex-1">
      <input
        ref={box}
        role="combobox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-autocomplete="list"
        aria-activedescendant={open && matches[active] ? `${listId}-${active}` : undefined}
        aria-label={label}
        className="ss-input w-full"
        placeholder="Pick a source…"
        value={open ? query : from ? `${from.name} [${from.unit}]` : ""}
        onClick={(e) => openAt(e.currentTarget)}
        onBlur={close}
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
          else if (e.key === "Escape" && open) close();
          else if (!open && e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
            // typing in a box that shows its source starts a new search
            setQuery(e.key);
            openAt(e.currentTarget);
          } else return;
          e.preventDefault();
        }}
      />
      {open && (
        <ul
          id={listId}
          role="listbox"
          aria-label={`Outputs for ${to.name}`}
          style={place}
          className="fixed z-50 overflow-y-auto overscroll-contain border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-lg"
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
            <li
              role="option"
              aria-selected={false}
              aria-disabled
              className="px-2 py-[2px] text-[11px] text-[color:var(--ss-text-dim)]"
            >
              No output matches.
            </li>
          )}
        </ul>
      )}
    </div>
  );
}

/** Connect several (UX-15): one output to the same input on every part of a
 *  type, or each unconnected input to the output of the same name. Lists
 *  the links it will make, and the inputs it leaves alone with the reason,
 *  then makes them all as one undo step. */
function BulkWiring({
  outputs,
  scope,
  onClose,
}: {
  outputs: End[];
  /** match names only to and from this part (the Selected part filter) */
  scope: { id: string; label: string } | null;
  onClose: () => void;
}) {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const unitGroups = useProjectStore((s) => s.unitGroups);
  const [mode, setMode] = useState<"all" | "names">("all");
  const [groupKey, setGroupKey] = useState("");
  const [sourceKey, setSourceKey] = useState<string | null>(null);
  const [replace, setReplace] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const index = useMemo(() => (project ? busIndex(project, libraryById) : null), [project, libraryById]);
  const groups = useMemo(() => (index ? inputGroups(index, libraryById) : []), [index, libraryById]);
  const group = groups.find((g) => g.key === groupKey) ?? groups[0];
  const source = outputs.find((o) => keyOf(o) === sourceKey) ?? null;
  const unitOf = (port: PortDef) => unitGroups[port.unitGroup ?? "No Unit"] ?? "-";
  const target: End | null = group
    ? { ...group.inputs[0], name: `every ${group.typeName} · ${group.portName}`, unit: unitOf(group.inputs[0].port) }
    : null;
  const busSource = source && index?.outputs.find((o) => keyOf(o) === keyOf(source));
  const plan: BulkPlan = !index
    ? { links: [], skipped: [] }
    : mode === "names"
      ? planMatchingNames(index, { elementId: scope?.id })
      : group && busSource
        ? planConnectToAll(index, busSource, group, { replace })
        : { links: [], skipped: [] };
  const what =
    mode === "names"
      ? `matching names${scope ? `, to and from ${scope.label}` : ""}`
      : `${source?.name} to every ${group?.typeName} · ${group?.portName}`;
  const differ =
    mode === "all" && source && target && source.unit !== target.unit && source.unit !== "-" && target.unit !== "-";
  const change = (fn: () => void) => {
    fn();
    setDone(null);
  };
  const radio = (value: "all" | "names", text: string) => (
    <label className="flex items-center gap-1">
      <input type="radio" name="bulk-mode" checked={mode === value} onChange={() => change(() => setMode(value))} />
      {text}
    </label>
  );

  const status =
    done ??
    (mode === "all" && !source && group
      ? "Pick the output to connect."
      : plan.links.length === 0 && (mode === "names" || source)
        ? "Nothing to connect."
        : "");

  // In a short panel this part takes the room before the rows below, and
  // scrolls (its buttons first).
  return (
    <section
      aria-label="Connect several"
      className="min-h-0 flex-initial space-y-1 overflow-y-auto border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-2 py-1.5 text-[11px]"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <div role="radiogroup" aria-label="How to connect" className="flex flex-wrap items-center gap-x-3 gap-y-1">
          {radio("all", "One output to every part of a type")}
          {radio("names", "Matching names")}
        </div>
        <button
          className="ss-toolbtn border border-[color:var(--ss-accent)] px-3 text-[color:var(--ss-accent)] disabled:opacity-40"
          disabled={plan.links.length === 0}
          onClick={() => {
            const n = applyBulkLinks(plan.links, what);
            setDone(`${countOf(n, "link")} connected. One Undo takes them all back.`);
          }}
        >
          Connect {countOf(plan.links.length, "input")}
        </button>
        <button className="ss-toolbtn border border-[color:var(--ss-border)] px-3" onClick={onClose}>
          Close
        </button>
        <span role="status" className="text-[color:var(--ss-text-dim)]">
          {status}
        </span>
      </div>
      {mode === "all" &&
        (group && target ? (
          <div className="flex flex-wrap items-center gap-1">
            <select
              className="ss-input"
              aria-label="Inputs to connect"
              value={group.key}
              onChange={(e) => change(() => setGroupKey(e.target.value))}
            >
              {groups.map((g) => (
                <option key={g.key} value={g.key}>
                  every {g.typeName} · {g.portName} ({g.inputs.length})
                </option>
              ))}
            </select>
            <span className="text-[color:var(--ss-text-dim)]">from</span>
            <div className="w-[260px] max-w-full">
              <SourcePicker
                to={target}
                from={source}
                outputs={outputs}
                label={`Output to connect to ${target.name}`}
                onPick={(o) => change(() => setSourceKey(keyOf(o)))}
              />
            </div>
            <label className="flex items-center gap-1">
              <input type="checkbox" checked={replace} onChange={(e) => change(() => setReplace(e.target.checked))} />
              Replace the sources they have
            </label>
          </div>
        ) : (
          <p className="text-[color:var(--ss-text-dim)]">No two parts of one type have a signal input yet.</p>
        ))}
      {mode === "names" && (
        <p className="text-[color:var(--ss-text-dim)]">
          Each input with no source gets the one output of the same name (a Script's <i>vehicle_speed</i> ← Vehicle ·
          Vehicle Speed){scope ? `, to and from ${scope.label} only (Selected part)` : ""}; not one of another unit or
          on the input's own part.
        </p>
      )}
      {differ && (
        <p className="text-[color:var(--ss-warn)]">
          {source!.name} is in {source!.unit}, {target!.name} in {target!.unit}: check that this is the signal you
          mean.
        </p>
      )}
      {(plan.links.length > 0 || plan.skipped.length > 0) && (
        <ul aria-label="Links to make" className="max-h-[160px] overflow-y-auto">
          {plan.links.map((l) => (
            <li key={keyOf(l.to)}>
              {l.from.name} → <b>{l.to.name}</b>
              {l.replaces.length > 0 && <span className="text-[color:var(--ss-text-dim)]"> (replaces its source)</span>}
            </li>
          ))}
          {plan.skipped.map((x) => (
            <li key={keyOf(x.to)} className="text-[color:var(--ss-text-dim)]">
              Not {x.to.name}: {x.why}.
            </li>
          ))}
        </ul>
      )}
    </section>
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
  const [bulk, setBulk] = useState(false);

  const { elements, rows, broken, outputs, portless } = useMemo(() => {
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
    const broken: Broken[] = [];
    const nameOf = (elementId: string, portId: string) =>
      ends.get(`${elementId}:${portId}`)?.name ??
      `${elements.find((e) => e.id === elementId)?.label ?? "?"} · ${portId}`;
    for (const d of project?.dataBusConnections ?? []) {
      const a = ends.get(`${d.element1Id}:${d.port1Id}`);
      const b = ends.get(`${d.element2Id}:${d.port2Id}`);
      const first = a?.port.direction === "output";
      const [from, to] = first ? [a, b] : [b, a];
      if (!from || !to || from.port.direction === to.port.direction) {
        const names = [nameOf(d.element1Id, d.port1Id), nameOf(d.element2Id, d.port2Id)];
        const why = from && to ? `both ${to.port.direction}s` : "a part or port is missing";
        broken.push({
          id: d.id,
          text: `${(first ? names : names.reverse()).join(" ↔ ")}: ${why}, so no data flows. Remove it.`,
          elementIds: [d.element1Id, d.element2Id],
        });
        continue;
      }
      const input = `${to.elementId}:${to.port.id}`;
      // (an input with two sources, made before 0.3, lists both)
      rows.push({ from, to, linkId: d.id, key: linked.has(input) ? d.id : input });
      linked.add(input);
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
    return { elements, rows, broken, outputs, portless };
  }, [project, libraryById, unitGroups]);

  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const hit = (text: string) => words.every((w) => text.toLowerCase().includes(w));
  const mine = (...ids: (string | undefined)[]) => !selectedOnly || ids.includes(selectedId ?? "");
  // Rows picked since the filters last changed stay, so a pick under
  // "Unconnected inputs" does not take the row (and the focus) away.
  const filters = JSON.stringify([query, freeOnly, selectedOnly, selectedId]);
  const [picked, setPicked] = useState({ filters, keys: [] as string[] });
  if (picked.filters !== filters) setPicked({ filters, keys: [] });
  const kept = picked.keys;
  const shown = rows.filter(
    (r) =>
      kept.includes(r.key) ||
      ((!freeOnly || !r.from) && mine(r.to.elementId, r.from?.elementId) && hit(`${r.from?.name ?? ""} ${r.to.name}`)),
  );
  const shownBroken = broken.filter((b) => !freeOnly && mine(...b.elementIds) && hit(b.text));
  const hints = portless.filter((el) => mine(el.id) && hit(el.label));
  const free = rows.filter((r) => !r.from).length;
  // why nothing is listed
  const sel = selectedOnly ? elements.find((e) => e.id === selectedId) : undefined;
  const empty = !rows.length
    ? "No part has a signal input yet."
    : selectedOnly && !sel
      ? "Select a part to see its signals."
      : sel && !query && !freeOnly
        ? outputs.some((o) => o.elementId === sel.id)
          ? `${sel.label} only has outputs, and they feed nothing yet: untick Selected part, then pick ${sel.label} as the source of an input.`
          : `${sel.label} has no signals.`
        : "No signal matches.";

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
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)] px-2"
          aria-expanded={bulk}
          title="Connect one output to every part of a type, or inputs to outputs of the same name"
          onClick={() => setBulk(!bulk)}
        >
          Connect several…
        </button>
        <span className="ml-auto text-[11px] text-[color:var(--ss-text-dim)]">
          {countOf(project?.dataBusConnections.length ?? 0, "link")} · {countOf(free, "unconnected input")}
        </span>
      </div>
      {bulk && (
        <BulkWiring
          outputs={outputs}
          scope={selectedOnly && sel ? { id: sel.id, label: sel.label } : null}
          onClose={() => setBulk(false)}
        />
      )}
      <ul className="min-h-0 flex-1 overflow-y-auto">
        {shownBroken.map((b) => (
          <li
            key={b.id}
            className="flex items-start gap-1 border-b border-[color:var(--ss-td-border)] px-2 py-[3px] text-[11px]"
          >
            <span className="min-w-0 flex-1 text-[color:var(--ss-warn)]">{b.text}</span>
            <button className="ss-toolbtn shrink-0" title="Remove connection" onClick={() => removeDataBus(b.id)}>
              <Trash2 size={12} />
            </button>
          </li>
        ))}
        {shown.map((r) => (
          <li
            key={r.key}
            className="flex items-start gap-1 border-b border-[color:var(--ss-td-border)] px-2 py-[3px] text-[11px]"
          >
            <SourcePicker
              to={r.to}
              from={r.from}
              outputs={outputs}
              onPick={(o) => {
                setPicked({ filters, keys: [...kept, r.key] });
                addDataBus(o.elementId, o.port.id, r.to.elementId, r.to.port.id, r.linkId ?? undefined);
              }}
            />
            <span className="mt-[3px] shrink-0 text-[color:var(--ss-text-dim)]" aria-hidden>
              →
            </span>
            <span className="mt-[3px] w-[45%] shrink-0 truncate" title={`${r.to.name} [${r.to.unit}]`}>
              <b>{r.to.name}</b> <span className="text-[color:var(--ss-text-dim)]">[{r.to.unit}]</span>
            </span>
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
        {shown.length + shownBroken.length + hints.length === 0 && (
          <li className="px-3 py-2 text-[11px] text-[color:var(--ss-text-dim)]">{empty}</li>
        )}
      </ul>
    </div>
  );
}
