// The Map block's own editor (docs/spec/tables.md): its inputs (the axes of
// its tables, in order, each with a unit and an outside-the-data rule), its
// outputs (a table each, all on one grid) and the tables. Every change to
// the ports changes the tables with them, in one undo step. A port's id is
// made once, when it is added: a rename changes its name only, so its wires,
// its table and its display unit stay where they are.
import { useState } from "react";
import unitsTable from "../../data/units.json";
import { Plus, Trash2 } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { alternatives } from "../../units";
import { addAxis, dropAxis, filled, gridKeys, unitGrid, type Grid } from "../../tableN";
import type { ElementInstance, MapBlockState, OutsidePolicy, PortDef } from "../../types";
import { OUTSIDE_LABELS, cellText, slugify } from "./gridRules";
import { TableNEditor, type SetGridUnit } from "./TableNEditor";
import { useTableImport, type DataApply } from "../ImportDataDialog";

export const MAP_DEF_ID = "signal.map";

const QUANTITIES = (unitsTable as unknown as { groups: Record<string, Record<string, unknown>> }).groups;
const WITHOUT = ["Fraction", "No Unit"];
const OTHER = "other";

/** The units a Map port can take, grouped by quantity: those of the
 *  library's unit groups first ("<group>|<unit>": the group stored, the unit
 *  shown), then every other unit the app knows, which the port keeps as its
 *  unit text, shown and never converted ("|<unit>"), then none. */
function unitOptions(unitGroups: Record<string, string>) {
  const taken = new Set<string>();
  const out: { label: string; options: { value: string; text: string }[] }[] = [];
  for (const [group, lib] of Object.entries(unitGroups)) {
    if (WITHOUT.includes(group)) continue;
    const units = alternatives(lib).length ? alternatives(lib) : [lib];
    units.forEach((u) => taken.add(u));
    out.push({ label: group, options: units.map((u) => ({ value: `${group}|${u}`, text: u })) });
  }
  for (const [quantity, units] of Object.entries(QUANTITIES)) {
    const rest = Object.keys(units).filter((u) => !taken.has(u) && u !== "-");
    if (rest.length)
      out.push({
        label: `${quantity[0].toUpperCase()}${quantity.slice(1)} (not converted)`,
        options: rest.map((u) => ({ value: `|${u}`, text: u })),
      });
  }
  out.push({
    label: "No unit",
    options: [
      { value: "Fraction|-", text: "Fraction (0-1)" },
      { value: "No Unit|-", text: "No unit" },
      { value: OTHER, text: "Another unit: type it…" },
    ],
  });
  return out;
}

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
  const options = unitOptions(unitGroups);
  const known = new Set(options.flatMap((g) => g.options.map((o) => o.value)));
  /** A port's numbers as the editor shows them: in its group's unit or the
   *  one chosen to show them in, or in the unit text typed for it, as it is. */
  const quantity = (p: PortDef, chosen?: string | null) =>
    p.unitText
      ? { lib: p.unitText, unit: p.unitText, fixed: true }
      : { lib: libOf(p), unit: shownOf(libOf(p), chosen) };
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
  // the name only: the id, and so the wires, the table and the display unit, stay
  const rename = (port: PortDef, name: string) => {
    const typed = name.trim();
    if (!typed || typed === port.name) return;
    save(ports.map((p) => (p.id === port.id ? { ...p, name: typed } : p)));
  };
  /** A unit picked for a port: another quantity keeps the numbers (they are
   *  read in it); a unit of the same quantity is only how they are shown, as
   *  the unit chooser's "Show in another unit". */
  const pickUnit = (port: PortDef, value: string) => {
    const [g, u] = value === OTHER ? ["No Unit", ""] : value.split("|");
    const group = g || "No Unit";
    // (a typed unit starts empty, unless the port already has one of its own)
    const own = port.unitText !== undefined && !known.has(`|${port.unitText}`) ? port.unitText : "";
    const text = value === OTHER ? own : g ? undefined : u;
    const lib = unitGroups[group] ?? "-";
    const display = text === undefined && u && u !== lib ? u : null;
    const same = group === (port.unitGroup ?? "No Unit") && port.unitText === undefined && text === undefined;
    const next = ports.map((p) => {
      if (p.id !== port.id) return p;
      const q: PortDef = { ...p, unitGroup: group };
      if (text === undefined) delete q.unitText;
      else q.unitText = text;
      return q;
    });
    const k = inputs.indexOf(port);
    const outs = { ...state.units.outputs };
    if (k < 0 && display) outs[port.id] = display;
    else delete outs[port.id];
    const axes = state.units.axes.map((x, i) => (i === k ? display : x));
    save(same ? ports : next, { units: { axes, outputs: outs } });
  };
  const setUnitText = (port: PortDef, text: string) =>
    text.trim() !== port.unitText &&
    save(ports.map((p) => (p.id === port.id ? { ...p, unitText: text.trim() } : p)));
  /** An import: the tables on the block's own ports, or (built from the
   *  data) new ports, their tables and the units to show them in. */
  const applyImport = (r: DataApply) => {
    if (!r.ports) return save(ports, { maps: r.tables as MapBlockState["maps"] });
    const next: PortDef[] = r.ports.map(({ display: _shown, ...p }) => p);
    const ins = r.ports.filter((p) => p.direction === "input");
    const outs = Object.fromEntries(
      r.ports.filter((p) => p.direction === "output" && p.display).map((p) => [p.id, p.display!]),
    );
    save(next, {
      maps: r.tables as MapBlockState["maps"],
      outside: ins.map((p) => state.outside[inputs.findIndex((q) => q.id === p.id)] ?? null),
      units: { axes: ins.map((p) => p.display), outputs: outs },
    });
  };
  const importer = useTableImport(
    {
      componentDefId: MAP_DEF_ID,
      paramKey: "maps",
      ports: ports.map((p) => ({
        id: p.id,
        name: p.name,
        direction: p.direction as "input" | "output",
        unitGroup: p.unitGroup,
        ...(p.unitText !== undefined && { unitText: p.unitText }),
      })),
    },
    applyImport,
  );
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

  const unitSelect = (p: PortDef, chosen: string | null | undefined) => {
    const value =
      p.unitText !== undefined
        ? known.has(`|${p.unitText}`)
          ? `|${p.unitText}`
          : OTHER
        : `${p.unitGroup ?? "No Unit"}|${shownOf(libOf(p), chosen)}`;
    return (
      <>
        <select
          className="ss-input w-[110px] text-[11px]"
          aria-label={`Unit of ${p.name}`}
          title="The unit of its numbers and of the signal wired to it (nothing is converted on the wire). Another unit of the same quantity only changes how the numbers are shown"
          value={value}
          onChange={(e) => pickUnit(p, e.target.value)}
        >
          {options.map((g) => (
            <optgroup key={g.label} label={g.label}>
              {g.options.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.text}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
        {value === OTHER && (
          <input
            className="ss-input w-[70px] text-[11px]"
            aria-label={`Unit text of ${p.name}`}
            placeholder="unit"
            title="A unit LightSim does not know: shown with the numbers, never converted"
            defaultValue={p.unitText}
            onBlur={(e) => setUnitText(p, e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
        )}
      </>
    );
  };
  const nameInput = (p: PortDef, what: string) => (
    <input
      key={p.id}
      className="ss-input min-w-0 flex-1 text-[11px]"
      aria-label={`Name of ${what} ${p.name}`}
      defaultValue={p.name}
      title="Its name, on the diagram and on its axis or table (a new name keeps its wires)"
      onBlur={(e) => rename(p, e.target.value)}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
    />
  );
  const head = "mb-1 flex items-center gap-2 text-[11px] font-semibold text-[color:var(--ss-text-dim)]";
  return (
    <div className="flex flex-col gap-2" data-map-editor>
      <div className="flex flex-wrap items-center gap-2 text-[11px]">
        {importer.tools}
        <span className="text-[color:var(--ss-text-dim)]">
          a map from Excel, CSV or text, in any layout; it can set the inputs and outputs too
        </span>
      </div>
      {importer.dialog}
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
            {unitSelect(p, state.units.axes[k])}
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
                        {p.name} {cellText(key, quantity(p).lib, quantity(p, state.units.axes[k]).unit)}
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
            {unitSelect(p, state.units.outputs[p.id])}
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
          axes={inputs.map((p, k) => ({ name: p.name, ...quantity(p, state.units.axes[k]) }))}
          outputs={outputs.map((p) => ({ key: p.id, name: p.name, ...quantity(p, state.units.outputs[p.id]) }))}
          tables={state.maps as Record<string, Grid>}
          onChange={(maps) => save(ports, { maps: maps as MapBlockState["maps"] })}
          setUnit={setUnit}
          onImportText={importer.onImportText}
        />
      ) : (
        <p className="text-[11px] text-[color:var(--ss-text-dim)]">
          Add at least one input and one output to fill in the map.
        </p>
      )}
    </div>
  );
}
