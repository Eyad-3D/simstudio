import { expect, test } from "vitest";
import { describeTyre, maxLoadKg, parseTyreCode, tyreValues } from "./tyre";

test("a metric code gives the ISO radius and the load index (MOD-48)", () => {
  const spec = parseTyreCode("205/55 R16 91V")!;
  expect(spec.unloadedRadiusM * 1000).toBeCloseTo(315.95, 2);
  expect(spec.loadIndex).toBe(91);
  expect(maxLoadKg(spec)).toBe(615);
  const values = tyreValues(spec);
  expect(values.radius_m).toBeCloseTo(0.3065, 4);
  expect(values.slip_stiffness).toBeGreaterThan(17.8);
  expect(values.slip_stiffness).toBeLessThan(19.5);
  expect(values.mu_nominal_load_N).toBeCloseTo(0.5 * 615 * 9.81, 1);
  expect(describeTyre(spec)).toContain("carries up to 615 kg (load index 91)");
});

test("other forms read, and what is not a code does not", () => {
  expect(parseTyreCode("225/45ZR17 94W XL")?.loadIndex).toBe(94);
  expect(parseTyreCode("195/75 R16C 107/105R")?.loadIndex).toBe(107);
  expect(parseTyreCode("20.5x7.0-13")!.unloadedRadiusM).toBeCloseTo(0.26035, 5);
  expect(tyreValues(parseTyreCode("20.5x7.0-13")!)).toEqual({ radius_m: 0.2525 });
  // typed key by key: a load index of one digit is not one yet
  expect(parseTyreCode("205/55 R16 9")).toBeNull();
  expect(parseTyreCode("205/55 R1")).toBeNull();
  expect(parseTyreCode("")).toBeNull();
});
