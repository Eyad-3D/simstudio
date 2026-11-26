import { describe, expect, it } from "vitest";
import type { CalibrationResult, LapPrediction } from "../api";
import { lacksPackPower } from "./CalibrateDialog";

const lap = (p: Partial<LapPrediction>): LapPrediction => ({
  status: "success",
  lap_time_log_s: 60,
  messages: [],
  ...p,
});
const result = (cal: Partial<LapPrediction>, check?: Partial<LapPrediction>): CalibrationResult => ({
  fit: { mu_scale: 0.9, cza: 2.5, rms_kmh: 3.4, evaluations: 99 },
  calibration_lap: lap(cal),
  ...(check ? { check_lap: lap(check) } : {}),
});

describe("Calibrate lap: the energy sentence (app pass #11)", () => {
  it("is left out when every lap's log has the pack power", () => {
    const both = { energy_model_kwh: 0.4, energy_log_kwh: 0.37, energy_error_pct: 9.28 };
    expect(lacksPackPower(result(both, both))).toBe(false);
  });
  it("is shown when a lap's log lacks it", () => {
    expect(lacksPackPower(result({ energy_model_kwh: 0.4 }))).toBe(true);
    expect(lacksPackPower(result({ energy_model_kwh: 0.4, energy_log_kwh: 0.37 }, { energy_model_kwh: 0.41 }))).toBe(
      true,
    );
  });
});
