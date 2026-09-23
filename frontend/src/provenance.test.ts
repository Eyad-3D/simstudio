import { describe, expect, it } from "vitest";
import { canonicalJson, modelFingerprint } from "./provenance";
import type { Project } from "./types";

const project = (overrides: Partial<Project> = {}): Project => ({
  id: "p",
  name: "P",
  systems: [{ id: "s", name: "S", parentId: null, elements: [], connections: [] }],
  dataBusConnections: [],
  cases: [{ id: "c", name: "Case", duration: 10, timeStep: 1 }],
  ...overrides,
});

describe("model fingerprints", () => {
  it("canonical JSON sorts keys at every level and keeps array order", () => {
    expect(canonicalJson({ b: 1, a: { d: [3, { f: 1, e: 2 }], c: null } })).toBe(
      '{"a":{"c":null,"d":[3,{"e":2,"f":1}]},"b":1}',
    );
  });

  it("is the SHA-256 of the canonical JSON, whatever the key order", async () => {
    // sha256 of '{"cases":[{"duration":10,"id":"c",…],…,"systems":[…"parentId":null}]}'
    const expected = "f97eb14893469539d561d4c954b33b8613eb8399ea531f5a004fe0bb9b135bdd";
    const reordered = Object.fromEntries(Object.entries(project()).reverse()) as unknown as Project;
    expect(await modelFingerprint(project())).toBe(expected);
    expect(await modelFingerprint(reordered)).toBe(expected);
  });

  it("changes when the model does", async () => {
    const edited = project({ cases: [{ id: "c", name: "Case", duration: 11, timeStep: 1 }] });
    expect(await modelFingerprint(edited)).not.toBe(await modelFingerprint(project()));
  });
});
