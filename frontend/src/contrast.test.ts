// GUI-02: the theme colours are readable in both themes. Every text colour is
// at least 4.5:1 on every surface it is drawn on, and field borders, wires,
// node icons and chart lines at least 3:1 (WCAG 2.2 AA 1.4.3 and 1.4.11).
// The tokens are read from index.css itself, so a changed token is checked
// without editing this file; a new text colour or surface goes in TEXT or
// SURFACES below, as a 6-digit hex token.
import { describe, expect, it } from "vitest";
import { KIND_COLOR } from "./components/canvas/ElementNode";
import { PALETTE } from "./components/panels/chartUtils";
import css from "./index.css?raw";

/** The --ss-* colours set in one rule of index.css (":root" or ".dark"). */
function tokens(rule: string): Record<string, string> {
  const block = css.split(`\n${rule} {`)[1].split("}")[0];
  return Object.fromEntries([...block.matchAll(/--ss-([\w-]+):\s*(#[0-9a-f]{6})\b/gi)].map((m) => [m[1], m[2]]));
}

/** WCAG contrast ratio of two #rrggbb colours. */
function contrast(a: string, b: string): number {
  const lum = (hex: string) =>
    [1, 3, 5]
      .map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
      .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
      .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const light = tokens(":root");
const THEMES = { light, dark: { ...light, ...tokens(".dark") } };
const TEXT = ["text", "text-dim", "accent", "ok", "warn", "err"];
// the chrome, panels and toolbars, a hovered row and a selected one
const SURFACES = ["chrome", "panel", "panel-alt", "hover", "accent-soft"];

for (const [theme, t] of Object.entries(THEMES)) {
  describe(`${theme} theme`, () => {
    it.each(TEXT.flatMap((fg) => SURFACES.map((bg) => [fg, bg])))("%s text on %s is at least 4.5:1", (fg, bg) =>
      expect(contrast(t[fg], t[bg])).toBeGreaterThanOrEqual(4.5),
    );
    it("white text on a filled button (accent-fill) is at least 4.5:1", () =>
      expect(contrast("#ffffff", t["accent-fill"])).toBeGreaterThanOrEqual(4.5));
    it.each(["chrome", "panel", "panel-alt"])("a field border on %s is at least 3:1", (bg) =>
      expect(contrast(t["field-border"], t[bg])).toBeGreaterThanOrEqual(3),
    );
    // the diagram and the charts are drawn on the panel colour
    it.each(Object.entries(KIND_COLOR))("%s wires and icons (%s) are at least 3:1 on the diagram", (_, c) =>
      expect(contrast(c, t.panel)).toBeGreaterThanOrEqual(3),
    );
    it.each(PALETTE)("chart line %s is at least 3:1", (c) => expect(contrast(c, t.panel)).toBeGreaterThanOrEqual(3));
  });
}
