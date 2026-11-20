import { expect, test } from "vitest";
import library from "./data/componentLibrary.json";
import table from "./data/units.json";
import {
  alternatives,
  cellReader,
  convert,
  convertProse,
  mapTable,
  reinterpret,
  shownUnit,
  showNumber,
  toStored,
} from "./units";
import type { ElementInstance } from "./types";

test("conversions use the import's table, offsets included", () => {
  expect(convert(100, "km/h", "m/s")).toBeCloseTo(27.7778, 4);
  expect(convert(60, "mph", "km/h")).toBeCloseTo(96.5606, 4);
  expect(convert(1000, "1/min", "rad/s")).toBeCloseTo(104.7198, 4);
  expect(convert(100, "kW", "hp")).toBeCloseTo(134.102, 3);
  // temperature needs an offset, not only a factor
  expect(convert(0, "°C", "°F")).toBeCloseTo(32, 12);
  expect(convert(100, "°C", "°F")).toBeCloseTo(212, 12);
  expect(convert(25, "°C", "K")).toBeCloseTo(298.15, 12);
  expect(convert(-40, "°F", "°C")).toBeCloseTo(-40, 12);
  expect(convert(0, "K", "°C")).toBeCloseTo(-273.15, 12);
  expect(convert(5, "°C", "°C")).toBe(5);
});

test("a parameter is offered the units of its quantity, only when stored in the library's unit for it", () => {
  expect(alternatives("km/h")).toEqual(["km/h", "m/s", "mph"]);
  expect(alternatives("°C")).toEqual(["°C", "K", "°F"]);
  expect(alternatives("kW")).toEqual(expect.arrayContaining(["kW", "W", "hp"]));
  expect(alternatives("1/min")).toEqual(expect.arrayContaining(["1/min", "rad/s"]));
  // no alternative: plain text
  expect(alternatives("%")).toEqual([]);
  expect(alternatives("-")).toEqual([]);
  expect(alternatives("1/(km/h)")).toEqual([]);
  // a PID gain in 1/s is not a rotational speed
  expect(alternatives("1/s")).toEqual([]);
  // every quantity's first unit is a unit the library uses, so no group is dead weight... and
  // every unit the library uses that has a group is that group's first unit
  const groups = (table as { groups: Record<string, Record<string, unknown>> }).groups;
  const firsts = new Set(Object.values(groups).map((g) => Object.keys(g)[0]));
  const used = new Set<string>();
  for (const c of (library as { components: { parameters: { unit: string; axes?: { unit: string }[] }[] }[] })
    .components)
    for (const p of c.parameters) for (const u of [p.unit, ...(p.axes ?? []).map((a) => a.unit)]) used.add(u);
  for (const u of firsts) expect(used.has(u) || u === "m/s" || u === "-", `${u} is used by the library`).toBe(true);
});

test("a stored value shows in another unit and reads back to exactly itself (no drift)", () => {
  const values = [0, 0.1, 1, 2.5, 7, 12.3, 60, 100, 120, 150, 310, 1927, 12000, 0.0085, 1e-6, -40, -273.15, 98.6];
  for (const g of Object.values((table as { groups: Record<string, Record<string, unknown>> }).groups)) {
    const [lib, ...others] = Object.keys(g);
    for (const unit of others)
      for (const v of values) {
        const text = showNumber(v, lib, unit);
        expect(toStored(Number(text), lib, unit), `${v} ${lib} shown in ${unit} as ${text}`).toBe(v);
        // the shortest text that reads back, so never noise such as 0.10000000000000002
        // (17 digits): an edit keeps 12 digits, so 13 are always enough
        expect(text.replace(/e.*$/, "").replace(/\D/g, "").replace(/^0+/, "").length, text).toBeLessThanOrEqual(13);
      }
  }
  // the library's own unit shows as before
  expect(showNumber(0.1, "km/h", "km/h")).toBe("0.1");
  expect(showNumber(0.30000000000000004, "kW", "kW")).toBe("0.30000000000000004");
  // no noise digits in another unit either
  expect(showNumber(0.36, "km/h", "m/s")).toBe("0.1");
  expect(showNumber(0, "°C", "°F")).toBe("32");
  expect(showNumber(100, "°C", "K")).toBe("373.15");
  // what the user types is converted once, without float noise
  expect(toStored(0.1, "km/h", "m/s")).toBe(0.36);
  expect(toStored(212, "°C", "°F")).toBe(100);
});

test("the chosen unit of a number, an axis and a table's values; anything unknown is the library's", () => {
  const el = {
    displayUnits: { v_max: { value: "mph" }, map: { value: "lbf·ft", axes: [null, "rad/s"] }, bad: { value: "kg" } },
  } as unknown as ElementInstance;
  expect(shownUnit(el, "v_max", "value", "km/h")).toBe("mph");
  expect(shownUnit(el, "map", "value", "N·m")).toBe("lbf·ft");
  expect(shownUnit(el, "map", 0, "V")).toBe("V");
  expect(shownUnit(el, "map", 1, "1/min")).toBe("rad/s");
  expect(shownUnit(el, "bad", "value", "km/h")).toBe("km/h"); // not a speed
  expect(shownUnit(el, "none", "value", "km/h")).toBe("km/h");
  expect(shownUnit({} as ElementInstance, "v_max", "value", "km/h")).toBe("km/h");
});

test("a table's axis or values map on their own, at any number of axes", () => {
  const t3 = { "1": { "10": { "100": 1, "200": 2 } }, "2": { "10": { "100": 3, "200": 4 } } };
  expect(mapTable(t3, 2, (n) => n / 100)).toEqual({ "1": { "10": { "1": 1, "2": 2 } }, "2": { "10": { "1": 3, "2": 4 } } });
  expect(mapTable(t3, 0, (n) => n * 10)).toEqual({ "10": t3["1"], "20": t3["2"] });
  expect(mapTable(t3, "value", (n) => -n)).toEqual({ "1": { "10": { "100": -1, "200": -2 } }, "2": { "10": { "100": -3, "200": -4 } } });
});

test("'the number is in another unit' keeps the numbers on screen and changes the values", () => {
  // shown in km/h (the library's unit), the numbers were really mph
  expect(reinterpret(100, "km/h", "km/h", "mph")).toBeCloseTo(160.9344, 10);
  // shown in m/s: the number on screen (10 m/s from 36 km/h) is really mph
  expect(reinterpret(36, "km/h", "m/s", "mph")).toBeCloseTo(16.09344, 10);
  // offsets: 20 shown in °C was really °F
  expect(reinterpret(20, "°C", "°C", "°F")).toBeCloseTo(-6.6667, 4);
  const table = { "1000": 100, "2000": 90 };
  const axis = mapTable(table, 0, (n) => reinterpret(n, "1/min", "1/min", "rad/s"));
  expect(Object.keys(axis).map(Number)[0]).toBeCloseTo(9549.2966, 3);
  expect(Object.values(axis)).toEqual([100, 90]);
});

test("a cell left as shown reads back to its stored value exactly; a typed one converts once", () => {
  const read = cellReader([0.30000000000000004, 100], "km/h", "mph");
  expect(read(showNumber(0.30000000000000004, "km/h", "mph"))).toBe(0.30000000000000004);
  expect(read(showNumber(100, "km/h", "mph"))).toBe(100);
  expect(read("10")).toBe(16.09344);
  const same = cellReader([0.1], "km/h", "km/h");
  expect(same("0.1")).toBe(0.1);
  expect(same("2")).toBe(2);
});

test("numbers with the library's unit in a help text speak the shown unit", () => {
  expect(convertProse("Car motors 200–450 N·m up to their base speed (150 kW).", "N·m", "lbf·ft")).toBe(
    "Car motors 147.5–331.9 lbf·ft up to their base speed (150 kW).",
  );
  expect(convertProse("small car 1,000–1,400 kg · 0.01–0.1 kg·m²", "kg", "lb")).toBe(
    "small car 2205–3086 lb · 0.01–0.1 kg·m²",
  );
  expect(convertProse("3-10 km/h.", "km/h", "m/s")).toBe("0.8333-2.778 m/s.");
  expect(convertProse("about 25 °C", "°C", "°F")).toBe("about 77 °F");
  expect(convertProse("no numbers", "km/h", "km/h")).toBe("no numbers");
});
