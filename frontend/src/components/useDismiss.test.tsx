import { act, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { useUIStore } from "../store/uiStore";
import { useDismiss } from "./useDismiss";

// tells React the updates below run inside act() on purpose
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Menu() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  useDismiss(open, () => setOpen(false), ref, button);
  return (
    <div ref={ref}>
      <button ref={button} onClick={() => setOpen(!open)}>
        Open
      </button>
      {open && <div role="menu" />}
    </div>
  );
}

let root: Root;
let pane: HTMLDivElement;
const menu = () => document.querySelector("[role=menu]");
const openMenu = () => act(() => document.querySelector("button")!.click());
// a press fires pointerdown, then mousedown, as in a browser
const press = (el: Element) =>
  act(() => {
    el.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
  });

beforeEach(() => {
  const host = document.body.appendChild(document.createElement("div"));
  root = createRoot(host);
  act(() => root.render(<Menu />));
  // stands in for React Flow's pane, which stops the events that reach it
  pane = document.body.appendChild(document.createElement("div"));
  for (const type of ["pointerdown", "mousedown"]) pane.addEventListener(type, (e) => e.stopPropagation());
});

afterEach(() => {
  act(() => root.unmount());
  document.body.innerHTML = "";
  useUIStore.getState().closeParamDialog();
});

test("a press outside closes the menu, even on an element that stops the event", () => {
  openMenu();
  press(menu()!);
  expect(menu()).not.toBeNull();
  press(pane);
  expect(menu()).toBeNull();
});

test("Esc closes the menu and gives the focus back to its button", () => {
  openMenu();
  document.body.focus();
  act(() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
  expect(menu()).toBeNull();
  expect(document.activeElement).toBe(document.querySelector("button"));
});

test("a dialog opening closes the menu", () => {
  openMenu();
  act(() => useUIStore.getState().openParamDialog("e1"));
  expect(menu()).toBeNull();
});
