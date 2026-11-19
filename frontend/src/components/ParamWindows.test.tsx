import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useParamWindows } from "../paramWindows";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import type { ComponentDef, Project } from "../types";
import { ParamWindows } from "./ParamWindows";

// tells React the updates below run inside act() on purpose
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// a window opened with window.open stands in as an iframe's window, which
// jsdom draws like any other; closing it removes the iframe
let opened: Window[] = [];
function fakeOpen() {
  return vi.spyOn(window, "open").mockImplementation((_url, name) => {
    const frame = document.body.appendChild(document.createElement("iframe"));
    const win = frame.contentWindow!;
    let closed = false;
    Object.defineProperty(win, "name", { value: name, configurable: true });
    Object.defineProperty(win, "closed", { get: () => closed, configurable: true });
    win.close = vi.fn(() => {
      // (jsdom closes a removed iframe's window itself: once is enough)
      if (closed) return;
      closed = true;
      frame.remove();
    });
    win.focus = vi.fn();
    opened.push(win);
    return win;
  });
}

const motorDef = {
  id: "test.motor",
  name: "Test Motor",
  icon: "motor",
  domain: "mechanical",
  ports: [],
  parameters: [
    { key: "power", label: "Peak Power", type: "number", default: 100, unit: "kW", description: "The most power." },
  ],
} as unknown as ComponentDef;

function project(id = "p1", labels: Record<string, string> = { m1: "Motor A", m2: "Motor B" }): Project {
  return {
    id,
    name: "P",
    cases: [],
    dataBusConnections: [],
    systems: [
      {
        id: "root",
        name: "Root",
        parentId: null,
        connections: [],
        elements: Object.entries(labels).map(([eid, label]) => ({
          id: eid,
          componentDefId: "test.motor",
          label,
          position: { x: 0, y: 0 },
          parameterOverrides: {},
        })),
      },
    ],
  } as unknown as Project;
}

let root: Root;
beforeEach(() => {
  opened = [];
  useProjectStore.setState({ project: project(), libraryById: { "test.motor": motorDef }, past: [], future: [] } as never);
  // the next edit is an undo step of its own, not part of the last test's
  useProjectStore.getState().beginHistory();
  root = createRoot(document.body.appendChild(document.createElement("div")));
  act(() => root.render(<ParamWindows />));
});

afterEach(() => {
  act(() => root.unmount());
  for (const w of useParamWindows.getState().windows) w.win.close();
  useParamWindows.setState({ windows: [] });
  useUIStore.getState().closeParamDialog();
  document.body.innerHTML = "";
  localStorage.clear();
});

const open = (id: string, key?: string) => act(() => useUIStore.getState().openParamDialog(id, key));
const powerInput = (win: Window) => win.document.querySelector<HTMLInputElement>('input[aria-label="Peak Power"]');
const overrides = (i = 0) => useProjectStore.getState().project!.systems[0].elements[i].parameterOverrides;

test("a part's parameters open in a window of their own, one per part", () => {
  const spy = fakeOpen();
  open("m1");
  // a page of its own: language, standards mode, the focus on the first field
  expect(opened[0].document.documentElement.lang).toBe("en");
  expect(opened[0].document.compatMode).toBe("CSS1Compat");
  expect(opened[0].document.activeElement?.getAttribute("aria-label")).toBe("Name");
  open("m2");
  expect(spy).toHaveBeenCalledTimes(2);
  expect(spy.mock.calls.map((c) => c[1])).toEqual(["lightsim-param-m1", "lightsim-param-m2"]);
  // no dialog in the main window: nothing is covered
  expect(useUIStore.getState().paramDialogId).toBeNull();
  expect(opened[0].document.title).toBe("Motor A");
  expect(opened[1].document.title).toBe("Motor B");
  expect(powerInput(opened[0])).not.toBeNull();
});

test("the form is drawn once the window has its stylesheet", async () => {
  fakeOpen();
  // a built page's stylesheet is a file: the window's copy of it loads later
  const link = document.head.appendChild(document.createElement("link"));
  link.rel = "stylesheet";
  try {
    open("m1");
    expect(powerInput(opened[0])).toBeNull();
    // asked for again meanwhile: still one window, nothing drawn yet
    open("m1");
    expect(opened).toHaveLength(1);
    expect(powerInput(opened[0])).toBeNull();
    await act(async () => {
      opened[0].document.head.querySelector("link")!.dispatchEvent(new Event("load"));
    });
    expect(powerInput(opened[0])).not.toBeNull();
  } finally {
    link.remove();
  }
});

test("asking again for an open part's window raises it instead of opening another", () => {
  const spy = fakeOpen();
  open("m1");
  open("m1");
  open("m1", "power");
  expect(spy).toHaveBeenCalledTimes(1);
  expect(useParamWindows.getState().windows).toHaveLength(1);
  // focused once on opening, then on each request
  expect(opened[0].focus).toHaveBeenCalledTimes(3);
  expect(useParamWindows.getState().windows[0]).toMatchObject({ paramKey: "power", seq: 1 });
});

test("an edit in the window is in the project at once, and an edit elsewhere shows in the window", () => {
  fakeOpen();
  open("m1");
  const input = powerInput(opened[0])!;
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setValue.call(input, "150");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(overrides().power).toBe(150);
  // the main window (or an undo, an AI agent's edit, an import) changes it
  act(() => useProjectStore.getState().setParameter("m1", "power", 175));
  expect(powerInput(opened[0])!.value).toBe("175");
  // (two edits of one field in a row are one undo step)
  act(() => useProjectStore.getState().undo());
  expect(powerInput(opened[0])!.value).toBe("100");
});

test("undo typed in the window undoes the project's last change", () => {
  fakeOpen();
  open("m1");
  act(() => useProjectStore.getState().setParameter("m1", "power", 175));
  const header = opened[0].document.querySelector("h1")!;
  act(() => {
    header.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }));
  });
  expect(overrides().power).toBeUndefined();
});

test("the title follows a rename", () => {
  fakeOpen();
  open("m1");
  act(() => useProjectStore.getState().renameElement("m1", "Front Motor"));
  expect(opened[0].document.title).toBe("Front Motor");
  expect(opened[0].document.querySelector("h1")?.textContent).toBe("Front Motor");
});

test("deleting the part closes its window, and only its window", () => {
  fakeOpen();
  open("m1");
  open("m2");
  act(() => useProjectStore.setState({ project: project("p1", { m2: "Motor B" }) }));
  expect(opened[0].close).toHaveBeenCalled();
  expect(opened[1].close).not.toHaveBeenCalled();
  expect(useParamWindows.getState().windows.map((w) => w.id)).toEqual(["m2"]);
});

test("another project, or none, closes every window", () => {
  fakeOpen();
  open("m1");
  open("m2");
  // another project whose parts happen to have the same ids
  act(() => useProjectStore.setState({ project: project("p2") }));
  expect(opened.map((w) => w.closed)).toEqual([true, true]);
  expect(useParamWindows.getState().windows).toEqual([]);
  open("m1");
  act(() => useProjectStore.setState({ project: null }));
  expect(useParamWindows.getState().windows).toEqual([]);
});

test("a Save As (the same project under a new id) keeps the windows", () => {
  fakeOpen();
  open("m1");
  const p = useProjectStore.getState().project!;
  act(() => useProjectStore.setState({ project: { ...p, id: "saved-as" } }));
  expect(useParamWindows.getState().windows).toHaveLength(1);
});

test("closing the window changes nothing in the project, and its size and place are kept", () => {
  fakeOpen();
  open("m1");
  const before = useProjectStore.getState().project;
  const win = opened[0];
  Object.defineProperty(win, "innerWidth", { value: 500, configurable: true });
  Object.defineProperty(win, "innerHeight", { value: 700, configurable: true });
  Object.defineProperty(win, "screenX", { value: 40, configurable: true });
  Object.defineProperty(win, "screenY", { value: 30, configurable: true });
  act(() => {
    win.dispatchEvent(new Event("pagehide"));
    win.close();
  });
  expect(useParamWindows.getState().windows).toEqual([]);
  expect(useProjectStore.getState().project).toBe(before);
  expect(JSON.parse(localStorage.getItem("lightsim-param-window")!)).toMatchObject({ width: 500, height: 700 });
  // the next window opens at that size
  const spy = fakeOpen();
  open("m1");
  expect(spy.mock.lastCall![2]).toBe("popup,width=500,height=700,left=40,top=30");
});

test("the window follows the theme, including a switch while it is open", () => {
  fakeOpen();
  const dark = useUIStore.getState().theme === "dark";
  open("m1");
  const html = opened[0].document.documentElement;
  expect(html.classList.contains("dark")).toBe(dark);
  act(() => useUIStore.getState().toggleTheme());
  return Promise.resolve().then(() => expect(html.classList.contains("dark")).toBe(!dark));
});

test("a blocked pop-up falls back to the dialog in the main window, said once in Messages", () => {
  vi.spyOn(window, "open").mockReturnValue(null);
  useProjectStore.setState({ messages: [] });
  open("m1");
  expect(useUIStore.getState().paramDialogId).toBe("m1");
  act(() => useUIStore.getState().closeParamDialog());
  open("m2", "power");
  expect(useUIStore.getState()).toMatchObject({ paramDialogId: "m2", paramDialogKey: "power" });
  const said = useProjectStore.getState().messages.filter((m) => /blocked the parameter window/.test(m.text));
  expect(said).toHaveLength(1);
});
