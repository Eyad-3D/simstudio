import { afterEach, describe, expect, it, vi } from "vitest";
import { clearDraft, loadDraft, saveDraft } from "./persist";
import type { Project } from "./types";

const DRAFT_KEY = "simstudio-draft-v1";

function project(name = "P"): Project {
  return {
    id: "p1",
    name,
    systems: [{ id: "sys", name, parentId: null, elements: [], connections: [] }],
    dataBusConnections: [],
    cases: [{ id: "case", name: "Case 1", duration: 10, timeStep: 1 }],
  };
}

afterEach(() => localStorage.clear());

describe("recovery draft", () => {
  it("round-trips the working copy, unsaved by default", () => {
    saveDraft(project("Edited"));
    const draft = loadDraft();
    expect(draft?.project.name).toBe("Edited");
    expect(draft?.clean).toBe(false);
    expect(draft?.savedAt).toBeTypeOf("number");
  });

  it("records a clean draft when nothing is unsaved", () => {
    saveDraft(project(), true);
    expect(loadDraft()?.clean).toBe(true);
  });

  it("the newest write wins, so a save turns an unsaved draft clean", () => {
    saveDraft(project("Edited"));
    saveDraft(project("Edited"), true);
    expect(loadDraft()?.clean).toBe(true);
  });

  it("treats drafts written before the flag existed as unsaved", () => {
    localStorage.setItem(DRAFT_KEY, JSON.stringify({ project: project(), savedAt: 1 }));
    const draft = loadDraft();
    expect(draft).not.toBeNull();
    expect(Boolean(draft?.clean)).toBe(false);
  });

  it("ignores missing, corrupt and non-project drafts", () => {
    expect(loadDraft()).toBeNull();
    localStorage.setItem(DRAFT_KEY, "{not json");
    expect(loadDraft()).toBeNull();
    localStorage.setItem(DRAFT_KEY, JSON.stringify({ project: { id: "x" }, savedAt: 1 }));
    expect(loadDraft()).toBeNull();
  });

  it("clearDraft removes it", () => {
    saveDraft(project());
    clearDraft();
    expect(loadDraft()).toBeNull();
  });

  it("never throws when storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("quota", "QuotaExceededError");
    });
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError");
    });
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError");
    });
    expect(() => saveDraft(project())).not.toThrow();
    expect(loadDraft()).toBeNull();
    expect(() => clearDraft()).not.toThrow();
  });
});
