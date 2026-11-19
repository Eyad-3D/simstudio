import { describe, expect, it } from "vitest";
import { csvBlob, csvText } from "./csv";

describe("CSV export (RES-37)", () => {
  it("keeps a label with a comma or a double quote in one column", () => {
    const csv = csvText([
      ["t_s", "Motor, rear · Power [W]", 'Pack "A" · Voltage [V]'],
      [0, 1.5, 2],
    ]);
    // three columns in both rows: the two labels are quoted, their quotes doubled
    expect(csv).toBe('t_s,"Motor, rear · Power [W]","Pack ""A"" · Voltage [V]"\n0,1.5,2');
  });

  it("starts the file with a byte-order mark so Excel reads UTF-8 (STD-09)", async () => {
    const bytes = new Uint8Array(await csvBlob([["T [N·m]"], [1]]).arrayBuffer());
    expect([...bytes.slice(0, 3)]).toEqual([0xef, 0xbb, 0xbf]);
    expect(new TextDecoder().decode(bytes.slice(3))).toBe("T [N·m]\n1");
  });
});
