import { expect, test } from "vitest";
import { sourceWords } from "./ValueSources";

test("a recorded source says its kind and how sure it is in words, not as a number", () => {
  expect(sourceWords({ source: "", kind: "generated", confidence: 0 })).toBe("Generated, not checked");
  expect(sourceWords({ source: "", kind: "measured", confidence: 2 })).toBe("Measured, source and method checked");
});
