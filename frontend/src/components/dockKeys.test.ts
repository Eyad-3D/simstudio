// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { DockviewComponent } from "dockview-core";
import { keepTabsOnDelete } from "./dockKeys";

// A real dockview with two panels in one group: its tab list closes the
// focused tab on Delete and Backspace unless the dock catches them first.
vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    unobserve() {}
    disconnect() {}
  },
);

function dock() {
  const root = document.createElement("div");
  document.body.append(root);
  const dv = new DockviewComponent(root, {
    createComponent: () => {
      const element = document.createElement("div");
      return { element, init: () => {} };
    },
  });
  dv.layout(800, 600);
  dv.addPanel({ id: "plot", component: "x", title: "Signal Plot" });
  dv.addPanel({ id: "problems", component: "x", title: "Problems" });
  const tab = (title: string) =>
    [...root.querySelectorAll<HTMLElement>(".dv-tab")].find((t) => t.textContent?.includes(title))!;
  return { root, dv, tab };
}

const press = (el: HTMLElement, key: string) =>
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));

afterEach(() => document.body.replaceChildren());

describe("panel tabs and the Delete key", () => {
  it("Delete and Backspace on a focused tab leave the panel open", () => {
    const { root, dv, tab } = dock();
    keepTabsOnDelete(root);
    press(tab("Signal Plot"), "Delete");
    press(tab("Problems"), "Backspace");
    expect(dv.panels.map((p) => p.id).sort()).toEqual(["plot", "problems"]);
  });

  it("without the guard dockview closes the tab (what the guard stops)", () => {
    const { dv, tab } = dock();
    press(tab("Signal Plot"), "Delete");
    expect(dv.panels.map((p) => p.id)).toEqual(["problems"]);
  });

  it("other keys still reach the tab list, and Delete in a text box is the box's", () => {
    const { root, tab } = dock();
    keepTabsOnDelete(root);
    const input = document.createElement("input");
    tab("Problems").append(input);
    const e = new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true });
    input.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
    const arrow = new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true, cancelable: true });
    tab("Problems").dispatchEvent(arrow);
    expect(arrow.defaultPrevented).toBe(true); // dockview moved the focus
  });
});
