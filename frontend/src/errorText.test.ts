import { expect, it } from "vitest";
import { detailText, failureText, isNotFound, refusalsOf, responseText } from "./errorText";
import { ApiError } from "./api";

// a project as a request sends it: the engine's validation errors point into it
const body = {
  project: {
    id: "p",
    name: "Car",
    systems: [
      {
        id: "root",
        name: "Car",
        parentId: null,
        elements: [
          { id: "el-1", componentDefId: "vehicle.body", label: "Vehicle", parameterOverrides: { mass_kg: "heavy" } },
          { id: "el-2", componentDefId: "battery.generic", label: "HV Battery", parameterOverrides: { capacity: 75 } },
        ],
        connections: [],
      },
    ],
    cases: [{ id: "c1", name: "Highway", duration: -1 }],
  },
};

it("turns the engine's validation errors into sentences naming the part and the parameter", () => {
  const detail = [
    { type: "float_parsing", loc: ["body", "project", "systems", 0, "elements", 0, "parameterOverrides", "mass_kg"], msg: "Input should be a valid number" },
    { type: "missing", loc: ["body", "project", "systems", 0, "elements", 1, "position"], msg: "Field required" },
    { type: "greater_than", loc: ["body", "project", "cases", 0, "duration"], msg: "Input should be greater than 0" },
  ];
  const text = detailText(detail, JSON.stringify(body));
  expect(text).not.toContain("[object Object]");
  expect(text).toBe(
    "3 problems: Vehicle · Vehicle Mass: Input should be a valid number; HV Battery · position: Field required; " +
      "Highway · duration: Input should be greater than 0.",
  );
  // one error: the sentence alone; a place the body does not have: its keys
  expect(detailText([detail[0]], body)).toBe("Vehicle · Vehicle Mass: Input should be a valid number");
  expect(detailText([{ loc: ["body", "x", 2], msg: "bad" }], {})).toBe("x · item 3: bad");
  // many: the first five, then how many more
  const many = Array.from({ length: 8 }, (_, i) => ({ loc: ["body", "cases", i], msg: `m${i}` }));
  expect(detailText(many)).toMatch(/^8 problems: .*m4; and 3 more\.$/);
});

it("takes a string, an object or nothing as the engine's detail, never printing [object Object]", () => {
  expect(detailText("Project 'x' not found")).toBe("Project 'x' not found");
  expect(detailText({ message: "The file is locked." })).toBe("The file is locked.");
  expect(detailText({ loc: ["body", "name"], msg: "Field required" })).toBe("name: Field required");
  expect(detailText({ code: "E1", file: "a.csv" })).toBe("code: E1; file: a.csv");
  expect(detailText([])).toBe("The engine refused the request without saying why.");
  expect(detailText(null)).toBe("");
});

it("reads a failed response's body, and says 404 and an unreachable engine plainly", async () => {
  const res = new Response(JSON.stringify({ detail: [{ loc: ["body", "project", "name"], msg: "Field required" }] }), {
    status: 422,
    statusText: "Unprocessable Entity",
  });
  expect(await responseText(res, body)).toBe("name: Field required");
  expect(await responseText(new Response("<html>", { status: 500, statusText: "Internal Server Error" }))).toBe(
    "Internal Server Error",
  );
  expect(isNotFound(new ApiError(404, "Run 'r' not found"))).toBe(true);
  expect(isNotFound(new Error("404 Not Found"))).toBe(true);
  expect(isNotFound(new ApiError(422, "404 lines"))).toBe(false);
  expect(failureText(new TypeError("Failed to fetch"))).toBe("the engine could not be reached");
  expect(failureText(new ApiError(422, "name: Field required"))).toBe("name: Field required");
});

it("makes a refused project's errors one row per value: the part, the parameter, what is wrong and what it got", () => {
  // an empty value: the engine tries each type a value may have, and says so once per type
  const at = ["body", "project", "systems", 0, "elements", 1, "parameterOverrides", "capacity_kWh"];
  const branches = ["bool", "int", "float", "str", "dict[str,union[float,...]]"].map((t) => ({
    type: `${t}_type`,
    loc: [...at, t],
    msg: "Input should be valid",
    input: null,
  }));
  const project = structuredClone(body);
  const p = project.project.systems[0].elements[1] as { componentDefId: string; parameterOverrides: Record<string, unknown> };
  p.parameterOverrides = { capacity_kWh: null };
  const rows = refusalsOf(
    [...branches, { type: "greater_than", loc: ["body", "project", "cases", 0, "duration"], msg: "Input should be greater than 0", input: -1 }],
    JSON.stringify(project),
  )!;
  expect(rows).toHaveLength(2);
  expect(rows[0]).toMatchObject({
    elementId: "el-2",
    part: "HV Battery",
    paramKey: "capacity_kWh",
    text: "Usable Capacity: must be a number (got nothing)",
  });
  // the engine's own words stay with the row, for a report
  expect(rows[0].code).toContain("float_type");
  expect(rows[0].code).toContain("parameterOverrides.capacity_kWh");
  // about no part: the place in words, no part or key
  expect(rows[1]).toMatchObject({ text: "Highway · duration: Input should be greater than 0 (got -1)" });
  expect(rows[1].elementId).toBeUndefined();
  // the text form lists each value once, too
  expect(detailText(branches, project)).toBe("HV Battery · Usable Capacity: must be a number (got nothing)");
  // a case's own value names the case, the part and the parameter
  const caseValue = refusalsOf(
    [{ type: "string_type", loc: ["body", "project", "cases", 0, "parameterOverrides", "el-1", "mass_kg"], msg: "Input should be a valid string", input: "" }],
    body,
  )!;
  expect(caseValue[0]).toMatchObject({ elementId: "el-1", paramKey: "mass_kg", text: "Highway · Vehicle Mass: Input should be a valid string (got '')" });
  // not a refusal of values
  expect(refusalsOf("Project 'x' not found")).toBeNull();
  expect(refusalsOf([{ msg: "no place" }])).toBeNull();
});
