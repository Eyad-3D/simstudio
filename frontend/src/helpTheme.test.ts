// The Help window and the Help panel open in the app's theme and follow a
// change of it (scripts/help-assets/theme.js), whatever the system is set to.
import { afterEach, describe, expect, it, vi } from "vitest";
import themeJs from "../scripts/help-assets/theme.js?raw";
import { THEME_KEY } from "./storageKeys";

const runOnHelpPage = () => new Function(themeJs)();

afterEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
  vi.resetModules();
});

describe("the help pages take the app's theme", () => {
  it("reads the theme the app saved, and follows the system when none is saved", () => {
    expect(themeJs).toContain(`"${THEME_KEY}"`);
    runOnHelpPage();
    expect(document.documentElement.dataset.theme).toBeUndefined();
    localStorage.setItem(THEME_KEY, "dark");
    runOnHelpPage();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("follows a change made in the app's window", () => {
    localStorage.setItem(THEME_KEY, "light");
    runOnHelpPage();
    expect(document.documentElement.dataset.theme).toBe("light");
    localStorage.setItem(THEME_KEY, "dark");
    window.dispatchEvent(new StorageEvent("storage", { key: THEME_KEY, newValue: "dark" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("the app saves its theme at start, so the help knows the default light too", async () => {
    await import("./store/uiStore");
    expect(localStorage.getItem(THEME_KEY)).toBe("light");
  });
});
