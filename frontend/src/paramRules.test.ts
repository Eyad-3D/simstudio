import { describe, expect, it } from "vitest";
import library from "./data/componentLibrary.json";
import { limitsText, rangeProblem } from "./paramRules";
import type { ComponentDef, ParameterDef } from "./types";

const components = library.components as unknown as ComponentDef[];
const param = (defId: string, key: string) =>
  components.find((c) => c.id === defId)!.parameters.find((p) => p.key === key) as ParameterDef;

describe("rangeProblem", () => {
  it("words a limit as Data Checks do", () => {
    // the same words backend/tests/test_data_checks.py expects
    expect(rangeProblem(param("battery.generic", "coulombic_efficiency_pct"), 0)).toBe(
      "must be above 0 and at most 100 %",
    );
    expect(rangeProblem(param("battery.generic", "capacity_Ah"), -1)).toBe("must be at least 0 and at most 100000 Ah");
    expect(rangeProblem(param("boundary.ambient", "pressure_kPa"), 0)).toBe("must be above 0 kPa");
    expect(rangeProblem(param("boundary.ambient", "temperature_C"), -300)).toBe(
      "must be above -273.15 and at most 1000 °C",
    );
    // a dimensionless number has no unit to name
    expect(rangeProblem(param("mech.gearbox", "default_gear"), 0)).toBe("must be at least 1");
  });

  it("keeps the edges", () => {
    const fill = param("fuel.tank", "initial_fill_pct");
    expect(rangeProblem(fill, 0)).toBeNull();
    expect(rangeProblem(fill, 100)).toBeNull();
    expect(rangeProblem(fill, 100.5)).not.toBeNull();
    expect(rangeProblem(param("battery.generic", "initial_soc_pct"), 0)).not.toBeNull();
    const b = param("vehicle.body", "road_load_b_N_per_kmh");
    expect(rangeProblem(b, -5)).toBeNull(); // free: a coast-down fit can give a negative B
    expect(limitsText(b)).toBeNull();
  });

  it("accepts every catalogue default", () => {
    for (const c of components)
      for (const p of c.parameters)
        if (p.type === "number") expect(rangeProblem(p, Number(p.default)), `${c.id} ${p.key}`).toBeNull();
  });
});
