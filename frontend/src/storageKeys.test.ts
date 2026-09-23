import { afterEach, describe, expect, it, vi } from "vitest";
import { migrateOldKeys } from "./storageKeys";

afterEach(() => localStorage.clear());

describe("settings saved under the old name (SimStudio)", () => {
  it("move to their new keys, and the old keys go", () => {
    localStorage.setItem("simstudio-theme", "dark");
    localStorage.setItem("simstudio-font-scale", "1.2");
    localStorage.setItem("simstudio-layout-v1", '{"version":4}');
    localStorage.setItem("simstudio-draft-v1", '{"project":{}}');
    localStorage.setItem("unrelated", "kept");
    migrateOldKeys(localStorage);
    expect({ ...localStorage }).toEqual({
      "lightsim-theme": "dark",
      "lightsim-font-scale": "1.2",
      "lightsim-layout-v1": '{"version":4}',
      "lightsim-draft-v1": '{"project":{}}',
      unrelated: "kept",
    });
  });

  it("never replace a setting already saved under the new name", () => {
    localStorage.setItem("simstudio-theme", "dark");
    localStorage.setItem("lightsim-theme", "light");
    migrateOldKeys(localStorage);
    expect(localStorage.getItem("lightsim-theme")).toBe("light");
    expect(localStorage.getItem("simstudio-theme")).toBeNull();
  });

  it("change nothing once moved", () => {
    localStorage.setItem("lightsim-theme", "dark");
    migrateOldKeys(localStorage);
    migrateOldKeys(localStorage);
    expect({ ...localStorage }).toEqual({ "lightsim-theme": "dark" });
  });

  it("stay under the old key when the copy fails", () => {
    localStorage.setItem("simstudio-draft-v1", "unsaved work");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("full", "QuotaExceededError");
    });
    migrateOldKeys(localStorage);
    expect(localStorage.getItem("simstudio-draft-v1")).toBe("unsaved work");
    expect(localStorage.getItem("lightsim-draft-v1")).toBeNull();
  });

  it("are moved before the UI reads them at start-up", async () => {
    localStorage.setItem("simstudio-theme", "dark");
    localStorage.setItem("simstudio-font-scale", "1.2");
    vi.resetModules();
    const { useUIStore } = await import("./store/uiStore");
    expect(useUIStore.getState().theme).toBe("dark");
    expect(useUIStore.getState().fontScale).toBe(1.2);
    expect(localStorage.getItem("simstudio-theme")).toBeNull();
  });
});
