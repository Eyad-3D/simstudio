import { describe, expect, it } from "vitest";
import libraryJson from "./data/componentLibrary.json";
import { codeOf, fingerprintOf } from "./trust";
import type { ComponentDef, ElementInstance, Project } from "./types";

const library = libraryJson as unknown as { components: ComponentDef[] };
const libraryById = Object.fromEntries(library.components.map((c) => [c.id, c]));

function project(overrides: Partial<Project> = {}): Project {
  const script: ElementInstance = {
    id: "el-hcu",
    componentDefId: "signal.script",
    label: "Hybrid Control Unit",
    position: { x: 0, y: 0 },
    parameterOverrides: { code: "def step(t, dt, inputs, state, params):\n    return {}\n" },
  };
  return {
    id: "p",
    name: "P",
    systems: [{ id: "root", name: "P", parentId: null, elements: [script], connections: [] }],
    dataBusConnections: [],
    cases: [{ id: "case-1", name: "City", duration: 10, timeStep: 0.1 }],
    ...overrides,
  } as Project;
}

describe("the code a project carries", () => {
  it("counts a script a case sets, not only the block's own (scripts asked for)", async () => {
    const plain = project();
    const sneaky = project({
      cases: [{
        id: "case-1", name: "City", duration: 10, timeStep: 0.1,
        parameterOverrides: { "el-hcu": { code: "import os\nos.remove('x')\n" } },
      }],
    });
    const a = codeOf(plain, libraryById, { scripts: true })!;
    const b = codeOf(sneaky, libraryById, { scripts: true })!;
    expect(b.text).toContain("os.remove");
    expect(b.text).toContain("case-1");
    expect(await fingerprintOf(a)).not.toBe(await fingerprintOf(b));
    expect(b.items).toEqual(["1 Script block (Python code)"]);
    // a case override for a part that is not a Script block is no code
    const other = project({ cases: [{ id: "c", name: "C", duration: 1, timeStep: 0.1, parameterOverrides: { "el-x": { code: "x" } } }] });
    expect(codeOf(other, libraryById, { scripts: true })!.text).toBe(codeOf(plain, libraryById, { scripts: true })!.text);
  });

  it("leaves Script blocks to their own review unless asked", () => {
    expect(codeOf(project(), libraryById)).toBeNull();
  });

  it("fingerprints the attached files as they are on disk, not as recorded", async () => {
    const attachments = [
      { path: "resources/motor.fmu", sha256: "a".repeat(64), bytes: 10 },
      { path: "resources/drive.csv", sha256: "c".repeat(64), bytes: 10 }, // data, not code
    ];
    const p = project({ attachments });
    const recorded = codeOf(p, libraryById)!;
    expect(recorded.items).toEqual(["motor.fmu"]);
    expect(recorded.changed).toEqual([]);
    // the same bytes on disk: the same fingerprint as before
    const same = codeOf(p, libraryById, { onDisk: { "resources/motor.fmu": "a".repeat(64) } })!;
    expect(await fingerprintOf(same)).toBe(await fingerprintOf(recorded));
    // a teammate's new motor.fmu, the project file unchanged: asks again
    const swapped = codeOf(p, libraryById, { onDisk: { "resources/motor.fmu": "b".repeat(64) } })!;
    expect(swapped.changed).toEqual(["resources/motor.fmu"]);
    expect(swapped.items).toEqual(["motor.fmu (changed since it was attached)"]);
    expect(await fingerprintOf(swapped)).not.toBe(await fingerprintOf(recorded));
    // a file missing on disk keeps its recorded hash (nothing of it can run)
    expect(codeOf(p, libraryById, { onDisk: {} })!.text).toBe(recorded.text);
  });
});
