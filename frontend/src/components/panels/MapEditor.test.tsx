// The Map block and a library table of three or more axes in the parameter
// window's form (docs/spec/tables.md): ports and tables change together,
// the grid shows a slice, breakpoints come and go on shown and held axes.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import libraryJson from "../../data/componentLibrary.json";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { gridKeys, type Grid } from "../../tableN";
import type { ComponentDef, ElementInstance, Project } from "../../types";
import { ParameterDialog } from "../ParameterDialog";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const raw = libraryJson as unknown as {
  components: ComponentDef[];
  awaitingEngine: ComponentDef[];
  unitGroups: Record<string, string>;
};
const lib = { ...raw, components: [...raw.components, ...raw.awaitingEngine] };
let root: Root;
let clip = "";
const clipboard = { writeText: vi.fn(), readText: vi.fn() };

/** A library part with a table of three axes (none ships yet). */
const cells: ComponentDef = {
  id: "test.cells",
  category: "Battery",
  name: "Cells",
  icon: "table",
  domain: "signal",
  ports: [],
  parameters: [
    {
      key: "r",
      label: "Cell Resistance",
      unit: "mΩ",
      type: "table",
      axes: [
        { name: "Temperature", unit: "°C", outside: "clamp" },
        { name: "SOC", unit: "%", outside: "clamp" },
        { name: "Current", unit: "A", outside: "clamp" },
      ],
      default: {
        "0": { "20": { "-100": 3.1, "100": 2.9 }, "80": { "-100": 2.6, "100": 2.4 } },
        "25": { "20": { "-100": 1.9, "100": 1.8 }, "80": { "-100": 1.5, "100": 1.4 } },
      },
    },
  ],
};

function open(element: Partial<ElementInstance> & { componentDefId: string }, dataBusConnections: unknown[] = []) {
  const def = [...lib.components, cells].find((c) => c.id === element.componentDefId)!;
  const el = { id: "p", label: "Part", position: { x: 0, y: 0 }, parameterOverrides: {}, ...element };
  useProjectStore.setState({
    project: {
      id: "x", name: "X", cases: [], dataBusConnections,
      systems: [{ id: "root", name: "Root", parentId: null, connections: [], elements: [el] }],
    } as unknown as Project,
    libraryById: { [def.id]: def },
    unitGroups: lib.unitGroups,
    past: [],
    future: [],
  } as never);
  useUIStore.getState().openParamDialog("p");
  act(() => root.render(<ParameterDialog />));
}

const part = () => useProjectStore.getState().project!.systems[0].elements[0];
const maps = () => part().parameterOverrides.maps as Record<string, Grid>;
const form = () => document.body;
const button = (text: string | RegExp, scope: ParentNode = form()) =>
  [...scope.querySelectorAll<HTMLButtonElement>("button")].find((b) =>
    typeof text === "string" ? b.textContent?.trim() === text : text.test(b.textContent?.trim() ?? ""),
  )!;
const press = (el: HTMLElement) => act(async () => el.click());
const setValue = (el: HTMLInputElement | HTMLSelectElement, value: string, event = "input") =>
  act(() => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event(event, { bubbles: true }));
  });
const choose = (label: string, value: string) =>
  setValue(form().querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`) ?? labelled(label), value, "change");
const labelled = (text: string) =>
  [...form().querySelectorAll("label")].find((l) => l.textContent?.trim().startsWith(text))!.querySelector("select")!;
const key = (el: Element, k: string) =>
  act(() => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));
const cell = (r: number, c: number) => form().querySelectorAll(".ss-grid tr")[r].querySelectorAll("td")[c];
const down = (el: Element, shiftKey = false) =>
  act(() => void el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, shiftKey })));
const status = () => form().querySelector('[role="status"]')?.textContent ?? "";

beforeEach(() => {
  root = createRoot(document.body.appendChild(document.createElement("div")));
  clip = "";
  clipboard.writeText.mockImplementation(async (t: string) => void (clip = t));
  clipboard.readText.mockImplementation(async () => clip);
  Object.defineProperty(window.navigator, "clipboard", { value: clipboard, configurable: true });
});
afterEach(() => {
  act(() => root.unmount());
  useUIStore.getState().closeParamDialog();
  document.body.innerHTML = "";
});

test("a Map block: inputs and outputs added share one grid, one breakpoint per input", async () => {
  open({ componentDefId: "signal.map" });
  expect(form().textContent).toContain("Add at least one input and one output");
  for (let i = 0; i < 3; i++) await press(button("Add input"));
  await press(button("Add output"));
  await press(button("Add output"));
  const el = part();
  expect(el.dynamicPorts!.map((p) => `${p.direction}:${p.id}`)).toEqual([
    "input:input_1", "input:input_2", "input:input_3", "output:output_1", "output:output_2",
  ]);
  expect(maps()).toEqual({ output_1: { "0": { "0": { "0": 0 } } }, output_2: { "0": { "0": { "0": 0 } } } });
  // each step was one undo step: the last output goes, the rest stays
  act(() => useProjectStore.getState().undo());
  expect(Object.keys(maps())).toEqual(["output_1"]);
});

/** A Map block of three inputs (2, 2 and 2 breakpoints) and two outputs,
 *  its output Eff wired to another part's input. */
function mapOf3() {
  const t = (base: number): Grid => ({
    "0": { "0": { "0": base, "100": base + 1 }, "50": { "0": base + 2, "100": base + 3 } },
    "10": { "0": { "0": base + 4, "100": base + 5 }, "50": { "0": base + 6, "100": base + 7 } },
  });
  open({
    componentDefId: "signal.map",
    dynamicPorts: [
      { id: "temp", name: "Temp", direction: "input", kind: "signal", unitGroup: "Temperature" },
      { id: "speed", name: "Speed", direction: "input", kind: "signal", unitGroup: "Rotational Speed" },
      { id: "torque", name: "Torque", direction: "input", kind: "signal", unitGroup: "Torque" },
      { id: "loss", name: "Loss", direction: "output", kind: "signal", unitGroup: "Power" },
      { id: "eff", name: "Eff", direction: "output", kind: "signal", unitGroup: "Percent" },
    ],
    parameterOverrides: { maps: { loss: t(0), eff: t(100) } },
  }, [{ id: "w", element1Id: "p", port1Id: "eff", element2Id: "other", port2Id: "sig_in" }]);
}

test("the grid shows a slice: rows and columns two inputs, the third held; a pasted value lands in it", async () => {
  mapOf3();
  // rows: the last input, columns: the one before, Temp held at its first breakpoint
  expect(cell(0, 0).textContent).toBe("Loss (kW)");
  expect([...form().querySelectorAll(".ss-grid tr")[0].querySelectorAll("td")].map((c) => c.textContent)).toEqual([
    "Loss (kW)", "0", "50",
  ]);
  expect(cell(2, 2).textContent).toBe("3");
  choose("Temp (°C)", "10");
  expect(cell(2, 2).textContent).toBe("7");
  // a value pasted at the selected cell changes that point of that slice only
  down(cell(1, 2));
  clip = "42";
  await press(button(/^Paste/));
  expect((maps().loss as Record<string, Record<string, Record<string, number>>>)["10"]["50"]["0"]).toBe(42);
  expect((maps().loss as Record<string, Record<string, Record<string, number>>>)["0"]["50"]["0"]).toBe(2);
  expect((maps().eff as Record<string, Record<string, Record<string, number>>>)["10"]["50"]["0"]).toBe(106);
  // the plot's line names the point in every axis
  down(cell(1, 2));
  expect(form().querySelector("[data-plot-mark]")?.textContent).toBe(
    "Marked: Temp 10 °C, Speed 50 1/min, Torque 0 N·m: Loss 42 kW",
  );
  // Copy takes the slice shown, with its headers
  down(cell(1, 1));
  await press(button(/^Copy/));
  expect(clip).toBe("Loss (kW)\t0\t50\n0\t4\t42\n100\t5\t7");
  // another output, the same slice
  choose("Output", "eff");
  expect(cell(1, 2).textContent).toBe("106");
});

test("a breakpoint on a held axis adds a whole slice copied from the nearest, and says so", async () => {
  mapOf3();
  await press(button("Add", form().querySelector(".ss-table-held")!));
  const input = form().querySelector<HTMLInputElement>('input[aria-label="New Temp breakpoint"]')!;
  setValue(input, "8");
  key(input, "Enter");
  expect(gridKeys(maps().loss, 3)[0]).toEqual(["0", "8", "10"]);
  expect((maps().eff as Record<string, Grid>)["8"]).toEqual((maps().eff as Record<string, Grid>)["10"]);
  expect(status()).toBe("Added Temp 8 °C: its points are a copy of 10's, in every output.");
  // it is the slice shown now; Remove takes it out again, in every output
  await press(button("Remove", form().querySelector(".ss-table-held")!));
  expect(gridKeys(maps().eff, 3)[0]).toEqual(["0", "10"]);
});

test("a breakpoint on a shown axis: a row added copies the last, rows deleted go from every slice", async () => {
  mapOf3();
  await press(button(/^Add Torque/));
  expect(gridKeys(maps().loss, 3)[2]).toEqual(["0", "100", "200"]);
  expect(cell(3, 1).textContent).toBe(cell(2, 1).textContent);
  down(cell(1, 1));
  await press(button(/^Rows$/));
  expect(gridKeys(maps().eff, 3)[2]).toEqual(["100", "200"]);
  // a breakpoint typed in a header moves it, in every output and slice
  down(cell(0, 2));
  clip = "60";
  await press(button(/^Paste/));
  expect(gridKeys(maps().eff, 3)[1]).toEqual(["0", "60"]);
});

test("removing an input keeps the slice chosen; outputs renamed and removed take their tables", async () => {
  mapOf3();
  await press(form().querySelector<HTMLButtonElement>('[aria-label="Remove input Temp"]')!);
  choose("Keep the values at", "10");
  await press(button("Remove", form().querySelector('section[aria-label="Inputs"]')!));
  expect(part().dynamicPorts!.filter((p) => p.direction === "input").map((p) => p.id)).toEqual(["speed", "torque"]);
  expect(maps().loss).toEqual({ "0": { "0": 4, "100": 5 }, "50": { "0": 6, "100": 7 } });
  // a renamed output keeps its id, so its table and its wire
  const eff = maps().eff;
  const name = form().querySelector<HTMLInputElement>('input[aria-label="Name of output Eff"]')!;
  act(() => name.focus());
  setValue(name, "Efficiency");
  act(() => name.blur());
  expect(part().dynamicPorts!.find((p) => p.id === "eff")?.name).toBe("Efficiency");
  expect(maps().eff).toEqual(eff);
  expect(useProjectStore.getState().project!.dataBusConnections.map((d) => d.port1Id)).toEqual(["eff"]);
  expect(cell(0, 0).textContent).toBe("Loss (kW)");
  choose("Output", "eff");
  expect(cell(0, 0).textContent).toBe("Efficiency (%)");
  await press(form().querySelector<HTMLButtonElement>('[aria-label="Remove output Loss"]')!);
  expect(Object.keys(maps())).toEqual(["eff"]);
  // the last output stays while there are inputs (the grid is in it)
  expect(form().querySelector<HTMLButtonElement>('[aria-label="Remove output Efficiency"]')!.disabled).toBe(true);
});

test("units per input and per output: shown in another unit, stored as they are", async () => {
  mapOf3();
  const speedUnit = [...form().querySelectorAll<HTMLButtonElement>("button")].find((b) =>
    b.getAttribute("aria-label")?.endsWith("unit of the Speed axis"),
  )!;
  await press(speedUnit);
  await press(button(/^rad\/s/));
  expect(part().displayUnits).toEqual({ maps: { axes: [null, "rad/s", null] } });
  expect(gridKeys(maps().loss, 3)[1]).toEqual(["0", "50"]);
  const valueUnit = [...form().querySelectorAll<HTMLButtonElement>("button")].find((b) =>
    b.getAttribute("aria-label")?.endsWith("unit of the Loss values"),
  )!;
  await press(valueUnit);
  await press(button(/^W\b/));
  expect(part().displayUnits?.maps?.outputs).toEqual({ loss: "W" });
  expect(cell(2, 2).textContent).toBe("3000");
});

test("a port's unit is picked from the units: the same quantity shows it, another keeps the numbers", () => {
  mapOf3();
  const port = (id: string) => part().dynamicPorts!.find((p) => p.id === id)!;
  const loss = maps().loss;
  // rad/s for Speed: still a rotational speed in 1/min, shown in rad/s at once
  choose("Unit of Speed", "Rotational Speed|rad/s");
  expect(port("speed").unitGroup).toBe("Rotational Speed");
  expect(part().displayUnits?.maps?.axes).toEqual([null, "rad/s", null]);
  expect(maps().loss).toEqual(loss);
  expect(cell(0, 2).textContent).toBe("5.23598775598");
  // W for Loss: shown in W
  choose("Unit of Loss", "Power|W");
  expect(part().displayUnits?.maps?.outputs).toEqual({ loss: "W" });
  expect(cell(2, 2).textContent).toBe("3000");
  // back to 1/min: the display unit goes
  choose("Unit of Speed", "Rotational Speed|1/min");
  expect(part().displayUnits?.maps?.axes).toBeUndefined();
  // another quantity keeps the numbers, read in the new unit: a force in N
  choose("Unit of Torque", "Force|N");
  expect(port("torque").unitGroup).toBe("Force");
  expect(maps().loss).toEqual(loss);
  // a unit with no unit group of its own is kept as text, shown as it is
  choose("Unit of Temp", "|kWh");
  expect(port("temp")).toMatchObject({ unitGroup: "No Unit", unitText: "kWh" });
  expect(form().querySelector(".ss-table-held")?.textContent).toContain("Temp (kWh)");
  // and one the app does not know is typed
  choose("Unit of Temp", "other");
  const text = form().querySelector<HTMLInputElement>('input[aria-label="Unit text of Temp"]')!;
  act(() => text.focus());
  setValue(text, "bar·s");
  act(() => text.blur());
  expect(port("temp").unitText).toBe("bar·s");
  expect(form().querySelector(".ss-table-held")?.textContent).toContain("Temp (bar·s)");
  expect(gridKeys(maps().loss, 3)[0]).toEqual(["0", "10"]);
  // the plot names it too
  down(cell(1, 1));
  expect(form().querySelector("[data-plot-mark]")?.textContent).toMatch(/^Marked: Temp 0 bar·s, /);
});

test("a library table of three axes: the same grid, its units per axis and for the values", async () => {
  open({ componentDefId: "test.cells" });
  expect(cell(0, 0).textContent).toBe("Cell Resistance (mΩ)");
  expect(cell(1, 1).textContent).toBe("3.1");
  choose("Temperature (°C)", "25");
  expect(cell(1, 1).textContent).toBe("1.9");
  // the rows now Temperature: SOC is held
  choose("Rows", "0");
  expect(form().querySelector(".ss-table-held")?.textContent).toContain("Current (A)");
  down(cell(2, 1));
  clip = "2";
  await press(button(/^Paste/));
  expect((part().parameterOverrides.r as Record<string, Record<string, Record<string, number>>>)["25"]["20"]["-100"]).toBe(2);
});
