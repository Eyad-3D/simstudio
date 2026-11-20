import { expect, it } from "vitest";
import { detailText, failureText, isNotFound, responseText } from "./errorText";
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
