import { describe, expect, it } from "vitest";
import { pinsFor, type FmuVariable } from "./fmu";
import type { ElementInstance } from "./types";

const v = (name: string, pin: FmuVariable["pin"], causality = pin ?? "parameter"): FmuVariable => ({
  name,
  valueReference: 0,
  type: "Real",
  causality,
  variability: "continuous",
  unit: "",
  start: 0,
  description: "",
  pin,
  settable: false,
});

const block = (ports: ElementInstance["dynamicPorts"] = []): ElementInstance => ({
  id: "f",
  componentDefId: "signal.fmu",
  label: "FMU",
  position: { x: 0, y: 0 },
  parameterOverrides: {},
  dynamicPorts: ports,
});

describe("FMU pins (STD-01)", () => {
  const vars = [v("u", "input"), v("ctrl.y", "output"), v("ctrl_y", "output"), v("k", null)];

  it("makes a pin per ticked variable, named after it, with a safe unique id", () => {
    const pins = pinsFor(block(), vars, new Set(["u", "ctrl.y", "ctrl_y", "k"]));
    expect(pins.map((p) => [p.name, p.direction])).toEqual([
      ["u", "input"],
      ["ctrl.y", "output"],
      ["ctrl_y", "output"],
    ]); // k is a parameter: never a pin
    expect(new Set(pins.map((p) => p.id)).size).toBe(3);
    for (const p of pins) expect(p.id).toMatch(/^fmu_[A-Za-z0-9_]+$/);
  });

  it("keeps the id of a pin that stays ticked, so its wires stay", () => {
    const first = pinsFor(block(), vars, new Set(["u", "ctrl.y"]));
    const renamedIds = first.map((p) => ({ ...p, id: `${p.id}_wired` }));
    const again = pinsFor(block(renamedIds), vars, new Set(["u", "ctrl.y", "ctrl_y"]));
    expect(again.find((p) => p.name === "u")?.id).toBe("fmu_u_wired");
    expect(again.find((p) => p.name === "ctrl.y")?.id).toBe("fmu_ctrl_y_wired");
    expect(again.find((p) => p.name === "ctrl_y")?.id).toBe("fmu_ctrl_y");
  });
});
