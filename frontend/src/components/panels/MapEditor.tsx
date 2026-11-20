// The Map block's own editor (docs/spec/tables.md): its inputs (the axes of
// its tables, in order, each with a unit and an outside-the-data rule), its
// outputs (a table each, all on one grid) and the tables. Every change to
// the ports changes the tables with them, in one undo step.
import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { alternatives } from "../../units";
import { addAxis, dropAxis, filled, gridKeys, unitGrid, type Grid } from "../../tableN";
import type { ElementInstance, MapBlockState, OutsidePolicy, PortDef } from "../../types";
import { OUTSIDE_LABELS, cellText, slugify } from "./gridRules";
import { TableNEditor, type SetGridUnit } from "./TableNEditor";

export const MAP_DEF_ID = "signal.map";

export function MapEditor({ element }: { element: ElementInstance }) {
  const setDynamicPorts = useProjectStore((s) => s.setDynamicPorts);
  const unitGroups = useProjectStore((s) => s.unitGroups);
  const ports = element.dynamicPorts ?? [];
  const inputs = ports.filter((p) => p.direction === "input");
  const outputs = ports.filter((p) => p.direction === "output");
  const state: MapBlockState = {
    maps: (element.parameterOverrides.maps ?? {}) as MapBlockState["maps"],
    outside: inputs.map((_, i) => element.tableOutside?.maps?.[i] ?? null),
    units: {
      axes: inputs.map((_, i) => element.displayUnits?.maps?.axes?.[i] ?? null),
      outputs: { ...element.displayUnits?.maps?.outputs },
    },
  };
  const [dropping, setDropping] = useState<{ id: string; keep: string } | null>(null);
  const libOf = (p: PortDef) => unitGroups[p.unitGroup ?? "No Unit"] ?? "-";
  const shownOf = (lib: string, u: string | null | undefined) => (u && alternatives(lib).includes(u) ? u : lib);
  const save = (next: PortDef[], change: Partial<MapBlockState> = {}) =>
    setDynamicPorts(element.id, next, { ...state, ...change });
  const freshId = (name: string, except?: string) => {
    let id = slugify(name);
    while (ports.some((p) => p.id === id && p.id !== except)) id += "_";
    return id;
  };
  const keys = gridKeys(outputs[0] ? (state.maps[outputs[0].id] ?? {}) : {}, inputs.length);
  const tables = (fn: (t: Grid) => Grid) =>
    Object.fromEntries(Object.entries(state.maps).map(([k, t]) => [k, fn(t)])) as MapBlockState["maps"];

  const add = (direction: "input" | "output") => {
    const list = direction === "input" ? inputs : outputs;
    let n = list.length + 1;
    const base = direction === "input" ? "Input" : "Output";
    while (ports.some((p) => p.id === slugify(`${base} ${n}`))) n += 1;
    const port: PortDef = { id: slugify(`${base} ${n}`), name: `${base} ${n}`, direction, kind: "signal", unitGroup: "No Unit" };
    // inputs first, in order, then the outputs, as the pins are drawn
    const next = direction === "input" ? [...inputs, port, ...outputs] : [...inputs, ...outputs, port];
    if (direction === "input")
      return save(next, {
        maps: tables(addAxis),
        outside: [...state.outside, null],
        units: { ...state.units, axes: [...state.units.axes, null] },
      });
    const like = outputs[0] ? state.maps[outputs[0].id] : undefined;
    save(next, { maps: { ...state.maps, [port.id]: like !== undefined ? filled(like, 0) : unitGrid(inputs.length) } });
  };

  const removeInput = (id: string, keep: string) => {
    const k = inputs.findIndex((p) => p.id === id);
    setDropping(null);
    save(
      ports.filter((p) => p.id !== id),
      {
        maps: tables((t) => dropAxis(t, k, keep)),
        outside: state.outside.filter((_, i) => i !== k),
        units: { ...state.units, axes: state.units.axes.filter((_, i) => i !== k) },
      },
    );
  };
  const removeOutput = (id: string) => {
    const maps = { ...state.maps };
    const units = { ...state.units.outputs };
    delete maps[id];
    delete units[id];
    save(ports.filter((p) => p.id !== id), { maps, units: { ...state.units, outputs: units } });
  };
  const rename = (port: PortDef, name: string) => {
    const typed = name.trim();
    if (!typed || typed === port.name) return;
    const id = freshId(typed, port.id);
    const next = ports.map((p) => (p.id === port.id ? { ...p, id, name: typed } : p));
    if (port.direction !== "output" || id === port.id) return save(next);
    const maps: MapBlockState["maps"] = {};
    for (const [k, t] of Object.entries(state.maps)) maps[k === port.id ? id : k] = t;
    const units = { ...state.units.outputs };
    if (units[port.id]) units[id] = units[port.id];
    delete units[port.id];
    save(next, { maps, units: { ...state.units, outputs: units } });
  };
  // a new unit keeps the numbers (they are read in it) and drops the unit chosen to show them in
  const regroup = (port: PortDef, group: string) => {
    const next = ports.map((p) => (p.id === port.id ? { ...p, unitGroup: group } : p));
    const k = inputs.indexOf(port);
    const outs = { ...state.units.outputs };
    delete outs[port.id];
    save(next, { units: { axes: state.units.axes.map((u, i) => (i === k ? null : u)), outputs: outs } });
  };
  const setOutside = (k: number, o: OutsidePolicy) =>
    save(ports, { outside: state.outside.map((x, i) => (i === k ? o : x)) });

  const setUnit: SetGridUnit = (slot, output, unit, again) => {
    const units = { axes: [...state.units.axes], outputs: { ...state.units.outputs } };
    if (slot === "value") {
      const port = outputs.find((p) => p.id === output)!;
      if (unit === libOf(port)) delete units.outputs[output];
      else units.outputs[output] = unit;
    } else units.axes[slot] = unit === libOf(inputs[slot]) ? null : unit;
    save(ports, { units, ...(again && { maps: { ...state.maps, ...again } as MapBlockState["maps"] }) });
  };

  const groupSelect = (p: PortDef) => (
    <select
      className="ss-input w-[130px] text-[11px]"
      aria-label={`Unit of ${p.name}`}
      title="The unit its numbers are in, and of the signal wired to it: nothing is converted on the wire"
      value={p.unitGroup ?? "No Unit"}
      onChange={(e) => regroup(p, e.target.value)}
    >
      {Object.entries(unitGroups).map(([group, unit]) => (
        <option key={group} value={group}>
          {group === "No Unit" ? "No unit" : unit === "-" || unit === group ? group : `${group} (${unit})`}
        </option>
      ))}
    </select>
  );
  const nameInput = (p: PortDef, what: string) => (
    <input
      key={p.id}
      className="ss-input min-w-0 flex-1 text-[11px]"
      aria-label={`Name of ${what} ${p.name}`}
      defaultValue={p.name}
      title="Its name, on the diagram and on its axis or table (renaming removes its wires)"
      onBlur={(e) => rename(p, e.target.value)}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
    />
  );
  const head = "mb-1 flex items-center gap-2 text-[11px] font-semibold text-[color:var(--ss-text-dim)]";
  return (
    <div className="flex flex-col gap-2" data-map-editor>
      <section aria-label="Inputs">
        <div className={head}>
          Inputs (the axes, in order)
          <button type="button" className="ss-toolbtn font-normal" onClick={() => add("input")}>
            <Plus size={12} aria-hidden /> Add input
          </button>
        </div>
        {inputs.length === 0 && (
          <p className="text-[11px] italic text-[color:var(--ss-text-dim)]">
            No input yet: each input is an axis of the map.
          </p>
        )}
        {inputs.map((p, k) => (
          <div key={p.id} className="flex flex-wrap items-center gap-1 py-0.5">
            {nameInput(p, "input")}
            {groupSelect(p)}
            <select
              className="ss-input text-[11px]"
              aria-label={`Outside the ${p.name} data`}
              title={`What the run does past the ${p.name} data`}
              value={state.outside[k] ?? "clamp"}
              onChange={(e) => setOutside(k, e.target.value as OutsidePolicy)}
            >
              {(Object.keys(OUTSIDE_LABELS) as OutsidePolicy[]).map((o) => (
                <option key={o} value={o}>
                  {OUTSIDE_LABELS[o]}
                </option>
              ))}
            </select>
            {dropping?.id === p.id ? (
              <span className="flex items-center gap-1 text-[11px]">
                <label className="flex items-center gap-1">
                  Keep the values at
                  <select
                    className="ss-input text-[11px]"
                    value={dropping.keep}
                    onChange={(e) => setDropping({ id: p.id, keep: e.target.value })}
                  >
                    {keys[k].map((key) => (
                      <option key={key} value={key}>
                        {p.name} {cellText(key, libOf(p), shownOf(libOf(p), state.units.axes[k]))}
                      </option>
                    ))}
                  </select>
                </label>
                <button type="button" className="ss-toolbtn" onClick={() => removeInput(p.id, dropping.keep)}>
                  Remove
                </button>
                <button type="button" className="ss-toolbtn" onClick={() => setDropping(null)}>
                  Cancel
                </button>
              </span>
            ) : (
              <button
                type="button"
                className="ss-toolbtn"
                aria-label={`Remove input ${p.name}`}
                title="Remove this input: the map keeps its values at one of its breakpoints"
                onClick={() =>
                  keys[k]?.length > 1 ? setDropping({ id: p.id, keep: keys[k][0] }) : removeInput(p.id, keys[k]?.[0] ?? "0")
                }
              >
                <Trash2 size={12} aria-hidden />
              </button>
            )}
          </div>
        ))}
      </section>
      <section aria-label="Outputs">
        <div className={head}>
          Outputs (a table each)
          <button type="button" className="ss-toolbtn font-normal" onClick={() => add("output")}>
            <Plus size={12} aria-hidden /> Add output
          </button>
        </div>
        {outputs.length === 0 && (
          <p className="text-[11px] italic text-[color:var(--ss-text-dim)]">No output yet: each output has a table.</p>
        )}
        {outputs.map((p) => (
          <div key={p.id} className="flex items-center gap-1 py-0.5">
            {nameInput(p, "output")}
            {groupSelect(p)}
            <button
              type="button"
              className="ss-toolbtn"
              aria-label={`Remove output ${p.name}`}
              disabled={outputs.length === 1 && inputs.length > 0}
              title={
                outputs.length === 1 && inputs.length > 0
                  ? "The map keeps its grid in its outputs' tables: add another output before removing this one"
                  : "Remove this output and its table"
              }
              onClick={() => removeOutput(p.id)}
            >
              <Trash2 size={12} aria-hidden />
            </button>
          </div>
        ))}
      </section>
      {inputs.length > 0 && outputs.length > 0 ? (
        <TableNEditor
          key={inputs.length}
          axes={inputs.map((p, k) => ({ name: p.name, lib: libOf(p), unit: shownOf(libOf(p), state.units.axes[k]) }))}
          outputs={outputs.map((p) => ({
            key: p.id,
            name: p.name,
            lib: libOf(p),
            unit: shownOf(libOf(p), state.units.outputs[p.id]),
          }))}
          tables={state.maps as Record<string, Grid>}
          onChange={(maps) => save(ports, { maps: maps as MapBlockState["maps"] })}
          setUnit={setUnit}
        />
      ) : (
        <p className="text-[11px] text-[color:var(--ss-text-dim)]">
          Add at least one input and one output to fill in the map.
        </p>
      )}
    </div>
  );
}
