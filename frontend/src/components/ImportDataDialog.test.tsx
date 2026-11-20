// The import in any layout (ImportDataDialog): what a paste does, the
// preview, Apply as one undo step, and a Map block built from the data. The
// engine's answers are canned; backend/tests/test_import_layouts.py checks
// the reading itself.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import libraryJson from "../data/componentLibrary.json";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import type { ComponentDef, ElementInstance, Project } from "../types";
import type { DataImport } from "../api";
import { ParameterDialog } from "./ParameterDialog";
import { importReason } from "./SpreadsheetGrid";

const importData = vi.hoisted(() => vi.fn());
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), importData }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const raw = libraryJson as unknown as {
  components: ComponentDef[];
  awaitingEngine: ComponentDef[];
  unitGroups: Record<string, string>;
};
const lib = [...raw.components, ...raw.awaitingEngine];
let root: Root;

function open(element: Partial<ElementInstance> & { componentDefId: string }) {
  const def = lib.find((c) => c.id === element.componentDefId)!;
  const el = { id: "p", label: "Part", position: { x: 0, y: 0 }, parameterOverrides: {}, ...element };
  useProjectStore.setState({
    project: {
      id: "x", name: "X", cases: [], dataBusConnections: [],
      systems: [{ id: "root", name: "Root", parentId: null, connections: [], elements: [el] }],
    } as unknown as Project,
    libraryById: { [def.id]: def },
    unitGroups: raw.unitGroups,
    past: [],
    future: [],
  } as never);
  useUIStore.getState().openParamDialog("p");
  act(() => root.render(<ParameterDialog />));
}

const part = () => useProjectStore.getState().project!.systems[0].elements[0];
const preview = () => document.querySelector<HTMLElement>("[data-import-preview]");
const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 0))));
const paste = async (grid: Element, text: string) => {
  const e = new Event("paste", { bubbles: true, cancelable: true }) as Event & { clipboardData: unknown };
  e.clipboardData = { getData: () => text };
  await act(async () => void grid.dispatchEvent(e));
  await settle();
};
const applyButton = () =>
  [...preview()!.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.startsWith("Apply"))!;

/** The engine's answer for the loss map pasted with titles. */
const LOSS: DataImport = {
  ok: true,
  layout: "A matrix (pivot table) (A2:C4)",
  fields: [
    { index: 0, title: "Speed [rpm]", name: "Speed", where: "row 2", role: "axis", fixed: true, unit: "1/min",
      written: "rpm", count: 2, slot: "axis:0", how: "name and unit", target: "Speed",
      options: ["1/min", "rad/s", "1/s"], used: "1/min", stored: "1/min" },
    { index: 1, title: "Torque [Nm]", name: "Torque", where: "column A", role: "axis", fixed: true, unit: "N·m",
      written: "Nm", count: 2, slot: "axis:1", how: "name and unit", target: "Torque",
      options: ["N·m", "kN·m", "lbf·ft"], used: "N·m", stored: "N·m" },
    { index: 2, title: "", name: "", where: "B3:C4", role: "value", fixed: true, unit: null, written: null,
      count: null, slot: "value:0", how: "only", target: "Power Loss", options: ["kW", "W"], used: "kW",
      stored: "kW", assumed: true },
  ],
  notes: [],
  problems: [],
  points: 4,
  holes: 0,
  filled: 0,
  targetAxes: [{ name: "Speed", unit: "1/min" }, { name: "Torque", unit: "N·m" }],
  targetValues: [{ key: "value", name: "Power Loss", unit: "kW" }],
  axes: [{ name: "Speed", unit: "1/min", count: 2, min: 1000, max: 2000 },
    { name: "Torque", unit: "N·m", count: 2, min: 10, max: 20 }],
  tables: { value: { "1000": { "10": 0.1, "20": 0.4 }, "2000": { "10": 0.2, "20": 0.5 } } },
  sheet: "Pasted text.txt",
  sheets: [{ name: "Pasted text.txt", tables: 1 }],
  blocks: [],
  block: null,
  sheetLabels: false,
  build: false,
  ports: null,
  source: "text",
  read: ["tab-separated, UTF-8, decimal comma"],
  decimal: "comma",
  decimalQuestion: null,
  cells: [],
  target: "Power Loss (Motor + Inverter)",
};

beforeEach(() => {
  root = createRoot(document.body.appendChild(document.createElement("div")));
  importData.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  useUIStore.getState().closeParamDialog();
  document.body.innerHTML = "";
});

test("what a paste needs the preview for, and what still goes into the cells", () => {
  const at = { r: 1, c: 1 };
  expect(importReason([["1", "2"], ["3", "4"]], at)).toBeNull();
  expect(importReason([["Speed [rpm]", "Loss [kW]"], ["1", "2"]], at)).toMatch(/titles or units \("Speed \[rpm\]"\)/);
  expect(importReason([["1,5", "2"]], at)).toMatch(/decimal comma/);
  expect(importReason([["1.000,5", "2"]], at)).toMatch(/decimal comma/);
  // a table with its own axes: at the grid's corner it is the grid's own layout
  const matrix = [["", "1000", "2000"], ["10", "1", "2"], ["20", "3", "4"]];
  expect(importReason(matrix, at)).toMatch(/its own axes/);
  expect(importReason(matrix, { r: 0, c: 0 })).toBeNull();
});

test("a paste with titles and units opens the preview; Apply is one undo step", async () => {
  importData.mockResolvedValue(LOSS);
  open({ componentDefId: "motor.emotor" });
  const before = part().parameterOverrides.power_loss;
  const grid = document.querySelector('[data-param="power_loss"] [tabindex]')!;
  const text = "Torque [Nm] \\ Speed [rpm]\t1000\t2000\n10\t0,1\t0,2\n20\t0,4\t0,5\n";
  await paste(grid, text);
  expect(importData).toHaveBeenCalledWith({ text }, { componentDefId: "motor.emotor", paramKey: "power_loss" }, {});
  expect(preview()!.textContent).toContain('Not pasted into the cells: the clipboard has titles or units ("Torque [Nm] \\ Speed [rpm]")');
  expect(preview()!.querySelector("[data-import-layout]")!.textContent).toBe(
    "Found: A matrix (pivot table) (A2:C4) · read as tab-separated, UTF-8, decimal comma",
  );
  // each axis with the title it was read from, its unit and conversion
  expect(preview()!.querySelector(".ss-import-fields")!.textContent).toContain("rpm = 1/min");
  expect(preview()!.textContent).toContain("no unit in the data: read in kW");
  // the table as it will be stored, in the editor's grid with its plot
  expect(preview()!.querySelector("[data-import-grid] .ss-grid")!.textContent).toContain("0.5");
  expect(part().parameterOverrides.power_loss).toEqual(before);
  await act(async () => applyButton().click());
  expect(preview()).toBeNull();
  expect(part().parameterOverrides.power_loss).toEqual(LOSS.tables!.value);
  act(() => useProjectStore.getState().undo());
  expect(part().parameterOverrides.power_loss).toEqual(before);
});

test("a plain block of numbers that fits still pastes into the cells", async () => {
  open({ componentDefId: "motor.emotor" });
  const grid = document.querySelector('[data-param="power_loss"] [tabindex]')!;
  const body = grid.querySelectorAll(".ss-grid tr")[1].querySelectorAll("td")[1];
  await act(async () => void body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })));
  await paste(grid, "7\t8\n");
  expect(importData).not.toHaveBeenCalled();
  expect(preview()).toBeNull();
  const t = part().parameterOverrides.power_loss as Record<string, Record<string, number>>;
  expect(Object.values(t).some((col) => Object.values(col).includes(7))).toBe(true);
});

test("a match the user changes goes to the engine, and the preview follows", async () => {
  importData.mockResolvedValue(LOSS);
  open({ componentDefId: "motor.emotor" });
  await paste(document.querySelector('[data-param="power_loss"] [tabindex]')!, "Speed [rpm] \\ Torque [Nm]\t1\n2\t3\n");
  const sel = preview()!.querySelector<HTMLSelectElement>('select[aria-label="What Speed [rpm] is"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(sel, "axis:1");
    sel.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await settle();
  expect(importData).toHaveBeenLastCalledWith(expect.anything(), expect.anything(), { fields: { 0: "axis:1" } });
});

test("a Map block is set from a file: its inputs, outputs, units and tables in one undo step", async () => {
  const built: DataImport = {
    ...LOSS,
    layout: "A list, a column per axis (A2:D9)",
    canBuild: true,
    build: true,
    fields: [],
    targetAxes: [],
    targetValues: [],
    ports: [
      { id: "temperature", name: "Temperature", direction: "input", kind: "signal", unitGroup: "Temperature", display: null },
      { id: "speed", name: "Speed", direction: "input", kind: "signal", unitGroup: "Rotational Speed", display: "rad/s" },
      { id: "loss", name: "Loss", direction: "output", kind: "signal", unitGroup: "Power", display: "W" },
      { id: "note", name: "Note", direction: "output", kind: "signal", unitGroup: "No Unit", unitText: "bar", display: null },
    ],
    tables: { loss: { "25": { "1000": 1 } }, note: { "25": { "1000": 2 } } },
  };
  importData.mockResolvedValue({ ...built, build: false, ok: false, ports: null, tables: undefined,
    problems: [{ level: "error", text: "The Map block has no inputs yet" }] });
  open({ componentDefId: "signal.map" });
  const file = new File(["T [°C];n [rad/s];Loss [W];Note [bar]\n25;104,72;1000;2\n"], "map.csv", { type: "text/csv" });
  const input = document.querySelector<HTMLInputElement>('[data-map-editor] input[type="file"]')!;
  Object.defineProperty(input, "files", { value: [file], configurable: true });
  await act(async () => void input.dispatchEvent(new Event("change", { bubbles: true })));
  await settle();
  expect(importData.mock.calls[0][0]).toEqual({ file: { name: "map.csv", data: expect.any(String) } });
  expect(importData.mock.calls[0][1]).toEqual({ componentDefId: "signal.map", paramKey: "maps", ports: [] });
  expect(applyButton().disabled).toBe(true);
  // the user's yes
  importData.mockResolvedValue(built);
  await act(async () => preview()!.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
  await settle();
  expect(importData).toHaveBeenLastCalledWith(expect.anything(), expect.anything(), { build: true, fields: undefined, units: undefined });
  expect(preview()!.textContent).toContain("(2 inputs, 2 outputs, named and with units from the titles)");
  await act(async () => applyButton().click());
  const el = part();
  expect(el.dynamicPorts).toEqual([
    { id: "temperature", name: "Temperature", direction: "input", kind: "signal", unitGroup: "Temperature" },
    { id: "speed", name: "Speed", direction: "input", kind: "signal", unitGroup: "Rotational Speed" },
    { id: "loss", name: "Loss", direction: "output", kind: "signal", unitGroup: "Power" },
    { id: "note", name: "Note", direction: "output", kind: "signal", unitGroup: "No Unit", unitText: "bar" },
  ]);
  expect(el.parameterOverrides.maps).toEqual(built.tables);
  expect(el.displayUnits?.maps).toEqual({ axes: [null, "rad/s"], outputs: { loss: "W" } });
  act(() => useProjectStore.getState().undo());
  expect(part().dynamicPorts ?? []).toEqual([]);
});
