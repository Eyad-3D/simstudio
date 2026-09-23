import { expect, it } from "vitest";

// Every module in src (tests aside) as source text, keyed "./path/file.ts".
const sources = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/** The src modules `file` imports at run time (`import type` is erased). */
function importsOf(file: string): string[] {
  const out: string[] = [];
  for (const m of sources[file].matchAll(/^(import|export)\s(?!type\s)[^;]*?from\s+"(\.{1,2}\/[^"]+)"/gm)) {
    const parts = [...file.split("/").slice(0, -1), ...m[2].split("/")];
    const path: string[] = [];
    for (const p of parts) {
      if (p === "..") path.pop();
      else if (p !== ".") path.push(p);
    }
    const base = `./${path.join("/")}`;
    const hit = [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`].find((f) => f in sources);
    if (hit) out.push(hit);
  }
  return out;
}

// A cycle makes load order decide which of two modules sees the other
// half-initialised (dialog.ts and the project store used to import each other).
it("no module imports itself back through other modules", () => {
  const cycles: string[] = [];
  const done = new Set<string>();
  const visit = (file: string, chain: string[]) => {
    const at = chain.indexOf(file);
    if (at >= 0) {
      cycles.push([...chain.slice(at), file].join(" → "));
      return;
    }
    if (done.has(file)) return;
    for (const next of importsOf(file)) visit(next, [...chain, file]);
    done.add(file);
  };
  expect(Object.keys(sources).length).toBeGreaterThan(20);
  for (const file of Object.keys(sources)) visit(file, []);
  expect(cycles).toEqual([]);
});
