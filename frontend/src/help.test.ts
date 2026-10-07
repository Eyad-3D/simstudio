import { afterEach, describe, expect, it } from "vitest";
import { closeHelp, useHelpStore, whatsNewOnce } from "./help";

afterEach(() => {
  localStorage.clear();
  closeHelp();
});

describe("What's new (LRN-09)", () => {
  it("stays quiet on a first start and remembers the version", () => {
    expect(whatsNewOnce("0.3.0")).toBe(false);
    expect(useHelpStore.getState().page).toBeNull();
    expect(localStorage.getItem("lightsim-last-version")).toBe("0.3.0");
  });

  it("opens the release notes once after an update", () => {
    localStorage.setItem("lightsim-last-version", "0.2.0");
    expect(whatsNewOnce("0.3.0")).toBe(true);
    expect(useHelpStore.getState().page).toBe("release-notes.html");
    closeHelp();
    expect(whatsNewOnce("0.3.0")).toBe(false);
    expect(useHelpStore.getState().page).toBeNull();
  });
});
