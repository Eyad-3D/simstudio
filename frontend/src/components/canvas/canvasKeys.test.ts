// UX-19: the diagram's keys, and when they belong to the diagram.
import { describe, expect, it } from "vitest";
import { canvasAction, diagramHasKeys, keyBelongsToTarget } from "./canvasKeys";

const press = (key: string, mods: Partial<Record<"ctrlKey" | "metaKey" | "altKey" | "shiftKey", boolean>> = {}) =>
  canvasAction({ key, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods });

describe("canvasAction", () => {
  it.each([
    ["a", "selectAll"],
    ["c", "copy"],
    ["x", "cut"],
    ["v", "paste"],
    ["d", "duplicate"],
    ["A", "selectAll"], // Caps Lock on
  ])("Ctrl+%s and Cmd+%s: %s", (key, action) => {
    expect(press(key, { ctrlKey: true })).toBe(action);
    expect(press(key, { metaKey: true })).toBe(action);
  });

  it("Delete, Backspace, F2 and Enter", () => {
    expect(press("Delete")).toBe("delete");
    expect(press("Backspace")).toBe("delete");
    expect(press("F2")).toBe("rename");
    expect(press("Enter")).toBe("properties");
  });

  it("leaves other keys and combinations alone", () => {
    expect(press("a")).toBeNull();
    expect(press("x")).toBeNull();
    expect(press("z", { ctrlKey: true })).toBeNull(); // undo is the app's
    expect(press("s", { ctrlKey: true })).toBeNull();
    expect(press("Enter", { ctrlKey: true })).toBeNull(); // runs the case
    expect(press("z", { ctrlKey: true, shiftKey: true })).toBeNull();
    expect(press("c", { ctrlKey: true, altKey: true })).toBeNull();
    expect(press("Enter", { shiftKey: true })).toBeNull();
    expect(press("Escape")).toBeNull();
  });
});

describe("keyBelongsToTarget", () => {
  const make = (html: string) => {
    document.body.innerHTML = html;
    return document.body.querySelector("[data-t]")!;
  };

  it("text fields, lists and editors keep every key", () => {
    for (const html of [
      `<input data-t>`,
      `<textarea data-t></textarea>`,
      `<select data-t></select>`,
    ]) {
      const t = make(html);
      expect(keyBelongsToTarget(t, "delete")).toBe(true);
      expect(keyBelongsToTarget(t, "selectAll")).toBe(true);
    }
    const editor = make(`<div data-t></div>`) as HTMLElement;
    Object.defineProperty(editor, "isContentEditable", { value: true });
    expect(keyBelongsToTarget(editor, "copy")).toBe(true);
  });

  it("Enter presses a focused button or link; other keys go to the diagram", () => {
    const button = make(`<button data-t>Zoom in</button>`);
    expect(keyBelongsToTarget(button, "properties")).toBe(true);
    expect(keyBelongsToTarget(button, "delete")).toBe(false);
    expect(keyBelongsToTarget(make(`<a href="#x"><span data-t>Help</span></a>`), "properties")).toBe(true);
    // a part on the diagram is a focusable div
    const part = make(`<div class="react-flow__node" tabindex="0" data-t></div>`);
    expect(keyBelongsToTarget(part, "properties")).toBe(false);
    expect(keyBelongsToTarget(part, "delete")).toBe(false);
    expect(keyBelongsToTarget(null, "delete")).toBe(false);
  });
});

describe("diagramHasKeys", () => {
  document.body.innerHTML = `<div id="diagram"><div id="part" tabindex="0"></div></div><button id="other"></button>`;
  const root = document.getElementById("diagram");
  const part = document.getElementById("part");
  const other = document.getElementById("other");
  const body = document.body;

  it("has them while the focus is in the diagram", () => {
    expect(diagramHasKeys(root, part, body, false, false)).toBe(true);
  });

  it("not while the focus is somewhere else, wherever the pointer is", () => {
    expect(diagramHasKeys(root, other, body, true, true)).toBe(false);
  });

  it("with the focus nowhere: after a click on the diagram, or with the pointer over it", () => {
    expect(diagramHasKeys(root, body, body, true, false)).toBe(true);
    expect(diagramHasKeys(root, null, body, false, true)).toBe(true);
    expect(diagramHasKeys(root, body, body, false, false)).toBe(false);
  });

  it("never without a diagram", () => {
    expect(diagramHasKeys(null, body, body, true, true)).toBe(false);
  });
});
