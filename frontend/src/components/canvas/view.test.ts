import { describe, expect, it } from "vitest";
import { fitPadding, refitAfterResize } from "./view";

describe("the diagram's view when its size changes", () => {
  it("the Signal Plot opened by the first run keeps the zoom, even when the model is cut off", () => {
    expect(refitAfterResize(true, true, true)).toBe(false);
    expect(refitAfterResize(false, true, true)).toBe(false);
  });
  it("otherwise a view the last fit left follows, and a model cut off is fitted again", () => {
    expect(refitAfterResize(false, true, false)).toBe(true);
    expect(refitAfterResize(true, false, false)).toBe(true);
    // a view the user set, still showing the whole model, stays
    expect(refitAfterResize(false, false, false)).toBe(false);
  });
});

describe("the room a fit leaves", () => {
  it("above the parts, the floating name and toolbar as drawn and the value over a part", () => {
    expect(fitPadding(40).top).toBe("64px");
    // the toolbar wrapped onto two rows in a narrow panel
    expect(fitPadding(70).top).toBe("94px");
    // never less than before
    expect(fitPadding(0).top).toBe("48px");
    expect(fitPadding(40).bottom).toBe("32px");
  });
});
