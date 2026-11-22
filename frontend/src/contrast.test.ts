// GUI-02: the theme colours are readable in both themes. Every text colour is
// at least 4.5:1 on every surface it is drawn on, and field borders, wires,
// node icons, pins, badges and chart lines at least 3:1 (WCAG 2.2 AA 1.4.3
// and 1.4.11). The tokens are read from index.css itself, so a changed token
// is checked without editing this file; a new text colour or surface goes in
// TEXT or SURFACES below, as a 6-digit hex token. GUI-14 moved the diagram's
// and the badges' colours into tokens too.
import { describe, expect, it } from "vitest";
import { KIND_COLOR } from "./components/canvas/ElementNode";
import { PALETTE } from "./components/panels/chartUtils";
import { SHADE_ALPHA, viridis } from "./components/gridTools";
import { SHOWN_ROW, STATUS_COLOR } from "./components/panels/runRows";
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
// text on a status's tint: badges (Properties' LIVE, an FMU's platform, a
// script's in and out), and the Messages rows of warnings and errors
const ON_TINT = [
  ["ok", "ok-soft"],
  ["warn", "warn-soft"],
  ["err", "err-soft"],
  ["info", "info-soft"],
  ...["text", "text-dim"].flatMap((fg) => [
    [fg, "warn-soft"],
    [fg, "err-soft"],
  ]),
];
// marks on the diagram (the panel colour) that must stand out at 3:1: a
// part's border, its warning and error badges (pins take their kind's colour,
// checked with the wires below)
const MARKS = ["node-border", "warn", "err"];

/** A colour as the app writes it: a hex value, or a token as var(--ss-…). */
function resolve(color: string, t: Record<string, string>): string {
  const token = color.match(/^var\(--ss-([\w-]+)\)$/)?.[1];
  if (!token) return color;
  expect(t[token], `--ss-${token} in index.css`).toBeDefined();
  return t[token];
}

for (const [theme, t] of Object.entries(THEMES)) {
  describe(`${theme} theme`, () => {
    it.each(TEXT.flatMap((fg) => SURFACES.map((bg) => [fg, bg])))("%s text on %s is at least 4.5:1", (fg, bg) =>
      expect(contrast(t[fg], t[bg])).toBeGreaterThanOrEqual(4.5),
    );
    it("white text on a filled button (accent-fill) is at least 4.5:1", () =>
      expect(contrast("#ffffff", t["accent-fill"])).toBeGreaterThanOrEqual(4.5));
    it.each(["warn-fill", "err-fill"])("white text on a filled badge (%s) is at least 4.5:1", (bg) =>
      expect(contrast("#ffffff", t[bg])).toBeGreaterThanOrEqual(4.5),
    );
    it.each(ON_TINT)("%s text on %s is at least 4.5:1", (fg, bg) =>
      expect(contrast(t[fg], t[bg])).toBeGreaterThanOrEqual(4.5),
    );
    it.each(["chrome", "panel", "panel-alt"])("a field border on %s is at least 3:1", (bg) =>
      expect(contrast(t["field-border"], t[bg])).toBeGreaterThanOrEqual(3),
    );
    // the diagram and the charts are drawn on the panel colour
    it.each(Object.entries(KIND_COLOR))("%s wires, pins and icons (%s) are at least 3:1 on the diagram", (_, c) => {
      expect(contrast(resolve(c, t), t.panel)).toBeGreaterThanOrEqual(3);
      expect(contrast(resolve(c, t), t["panel-alt"])).toBeGreaterThanOrEqual(3);
    });
    // the kinds are told apart (UI 4.42): every two at least 15 apart in
    // CIELAB, in normal vision and with protanopia, deuteranopia and tritanopia
    it.each(Object.keys(DEFICIENCY))("the kinds of connection stay apart in %s", (vision) => {
      const kinds = Object.entries(KIND_COLOR).filter(([k]) => k !== "power"); // (power is drawn as electrical)
      for (const [i, [a, ca]] of kinds.entries())
        for (const [b, cb] of kinds.slice(i + 1)) {
          const d = deltaE(seen(resolve(ca, t), vision), seen(resolve(cb, t), vision));
          expect(d, `${a} and ${b}`).toBeGreaterThanOrEqual(15);
        }
    });
    it.each(MARKS)("%s is at least 3:1 on the diagram", (c) =>
      expect(contrast(t[c], t.panel)).toBeGreaterThanOrEqual(3),
    );
    // a battery pin's + and − are text
    it.each(["pole-positive", "pole-negative"])("%s marks are at least 4.5:1 on a part", (c) =>
      expect(contrast(t[c], t.panel)).toBeGreaterThanOrEqual(4.5),
    );
    // the runs table's status cell on a plain, a hovered and the shown row
    it.each(
      Object.entries(STATUS_COLOR).flatMap(([s, c]) =>
        ["var(--ss-panel)", "var(--ss-hover)", SHOWN_ROW].map((bg) => [s, c, bg]),
      ),
    )("a run's status %s (%s) on %s is at least 4.5:1", (_, c, bg) =>
      expect(contrast(resolve(c, t), resolve(bg, t))).toBeGreaterThanOrEqual(4.5),
    );
    it.each(PALETTE)("chart line %s is at least 3:1", (c) => expect(contrast(c, t.panel)).toBeGreaterThanOrEqual(3));
    // a table's cells shaded by value (gridTools.ts): the number stays
    // readable on every step of the scale, on both row colours, selected or not
    it.each(["panel", "panel-alt"])("a shaded value on %s is at least 4.5:1", (bg) => {
      for (let i = 0; i <= 20; i++) {
        const shaded = over(viridis(i / 20), SHADE_ALPHA, t[bg]);
        expect(contrast(t.text, shaded), `step ${i}`).toBeGreaterThanOrEqual(4.5);
        expect(contrast(t.text, over(hexRgb(t.accent), 0.18, shaded)), `step ${i}, selected`).toBeGreaterThanOrEqual(4.5);
      }
    });
  });
}

/** Colour vision, as linear-RGB matrices: Machado, Oliveira and Fernandes
 *  (2009), severity 1, for the three common dichromacies. */
const DEFICIENCY: Record<string, number[][]> = {
  "normal vision": [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
  ],
  protanopia: [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
  ],
  deuteranopia: [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.01182, 0.04294, 0.968881],
  ],
  tritanopia: [
    [1.255528, -0.076749, -0.178779],
    [-0.078411, 0.930809, 0.147602],
    [0.004733, 0.691367, 0.3039],
  ],
};

/** A #rrggbb colour as seen with `vision`, in CIELAB (D65). */
function seen(hex: string, vision: string): number[] {
  const lin = hexRgb(hex).map((c) => c / 255).map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  const [r, g, b] = DEFICIENCY[vision].map((row) => Math.min(1, Math.max(0, row.reduce((s, m, i) => s + m * lin[i], 0))));
  const f = (x: number) => (x > 0.008856 ? Math.cbrt(x) : 7.787 * x + 16 / 116);
  const [x, y, z] = [
    (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047,
    0.2126 * r + 0.7152 * g + 0.0722 * b,
    (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883,
  ].map(f);
  return [116 * y - 16, 500 * (x - y), 200 * (y - z)];
}

/** CIE76 colour difference. */
const deltaE = (p: number[], q: number[]) => Math.hypot(...p.map((v, i) => v - q[i]));

function hexRgb(hex: string): number[] {
  return [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
}
/** `rgb` laid at `alpha` over the colour `bg`, as #rrggbb. */
function over(rgb: number[], alpha: number, bg: string): string {
  return `#${hexRgb(bg)
    .map((b, i) => Math.round(rgb[i] * alpha + b * (1 - alpha)).toString(16).padStart(2, "0"))
    .join("")}`;
}
