import { describe, expect, it } from "vitest";
import { csvText } from "./csv";

describe("CSV export (RES-37)", () => {
  it("keeps a label with a comma or a double quote in one column", () => {
    const csv = csvText([
      ["t_s", "Motor, rear · Power [W]", 'Pack "A" · Voltage [V]'],
      [0, 1.5, 2],
    ]);
    // three columns in both rows: the two labels are quoted, their quotes doubled
    expect(csv).toBe('t_s,"Motor, rear · Power [W]","Pack ""A"" · Voltage [V]"\n0,1.5,2');
  });
});
