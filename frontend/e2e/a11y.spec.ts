// Accessibility gate (axe-core) on three screens: the topology workspace, the
// Results page and a parameter dialog, each in the light and the dark theme.
// It fails on any serious or critical violation that is not in
// a11y-baseline.json, the list of issues the app had when the gate was
// introduced, kept per screen and theme ("topology-dark"). Fixing one of
// those is reported (so the baseline can shrink) but never fails the test.
//
// Baseline entries are "rule-id  element", where the element is named by what
// a user sees on it (see describeElement), not by axe's CSS selector: those
// selectors change with any layout or class change, so a baseline of them
// would fail on unrelated UI work and teach people to regenerate it blindly.
//
// Regenerate the baseline after a deliberate change:
//   UPDATE_A11Y_BASELINE=1 npx playwright test a11y
//
// Colour contrast is the exception (GUI-02): a sweep over every panel, menu
// and dialog in both themes fails on any text below the WCAG AA minimum, and
// on any color-contrast line in the baseline.
import AxeBuilder from "@axe-core/playwright";
import { readFileSync, writeFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { drawnLines, openApp, ribbonTab, runActiveCase, runButton, selectElement, showPanel } from "./app";

type Baseline = Record<string, string[]>;
const BASELINE_FILE = new URL("./a11y-baseline.json", import.meta.url);
const baseline: Baseline = JSON.parse(readFileSync(BASELINE_FILE, "utf8"));
const updating = Boolean(process.env.UPDATE_A11Y_BASELINE);
const found: Baseline = {};

/** Names a failing element (runs in the page): its tag and role, and the
 *  words a user sees on it (its text; for a form field its label, title or
 *  placeholder, else the table row or field it sits in), with digits as "#"
 *  so counts and values do not matter. An element with no words gets no
 *  name, and axe's selector is used instead. */
function describeElement(selector: string): string | null {
  const el = document.querySelector(selector);
  if (!el) return null;
  const tag = el.tagName.toLowerCase();
  const role = el.getAttribute("role");
  const type = tag === "input" ? `[type=${el.getAttribute("type") ?? "text"}]` : "";
  const hint = el.getAttribute("aria-label") || el.getAttribute("title") || el.getAttribute("placeholder");
  const text = (node: Element | null | undefined) =>
    node && ("innerText" in node ? (node as HTMLElement).innerText : node.textContent);
  const words = /^(input|select|textarea)$/.test(tag)
    ? hint || text(el.closest("tr")?.querySelector("td, th")) || text(el.parentElement)
    : text(el) || hint;
  const tidy = (words ?? "").replace(/\d+(?:[.,]\d+)?/g, "#").replace(/\s+/g, " ").trim();
  return tidy ? `${tag}${role ? `[role=${role}]` : ""}${type} "${tidy.slice(0, 40)}"` : null;
}

/** dockview draws a 1-px separator as a ::before of every panel, which makes
 *  axe give up on the colour of all text inside panels ("incomplete"); hide
 *  it so the panels' text is checked too. */
async function showPanelText(page: Page) {
  await page.addStyleTag({ content: ".dv-view::before { content: none !important; }" });
}

/** Serious/critical violations on the page: "rule-id  element" → axe's
 *  selector for it (to find it when it is new). */
async function blockingViolations(page: Page): Promise<Map<string, string>> {
  await showPanelText(page);
  const { violations } = await new AxeBuilder({ page }).analyze();
  const keys = new Map<string, string>();
  for (const v of violations) {
    if (v.impact !== "serious" && v.impact !== "critical") continue;
    for (const n of v.nodes) {
      const selector = n.target.join(" ");
      const [only] = n.target;
      const element =
        n.target.length === 1 && typeof only === "string"
          ? await page.evaluate(describeElement, only)
          : null;
      keys.set(`${v.id}  ${element ?? selector}`, selector);
    }
  }
  return keys;
}

async function check(page: Page, screen: string) {
  const current = await blockingViolations(page);
  if (updating) {
    found[screen] = [...current.keys()].sort();
    return;
  }
  const known = new Set(baseline[screen] ?? []);
  const fixed = [...known].filter((k) => !current.has(k));
  if (fixed.length) {
    test.info().annotations.push({
      type: "a11y-fixed",
      description: `no longer found on ${screen}, remove from a11y-baseline.json: ${fixed.join(" | ")}`,
    });
  }
  const added = [...current].filter(([k]) => !known.has(k)).map(([k, at]) => `${k}   (axe: ${at})`);
  expect(added, `new serious/critical axe violations on the ${screen} screen`).toEqual([]);
}

test.afterAll(() => {
  if (updating) writeFileSync(BASELINE_FILE, JSON.stringify({ ...baseline, ...found }, null, 2) + "\n");
});

/** Text on the page below WCAG AA contrast, as "ratio  text  fg on bg". */
async function contrastFailures(page: Page): Promise<string[]> {
  await showPanelText(page);
  const { violations } = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  return violations.flatMap((v) =>
    v.nodes.map((n) => {
      const d = n.any[0]?.data as { contrastRatio?: number; fgColor?: string; bgColor?: string } | undefined;
      return `${d?.contrastRatio}  ${n.target.join(" ")}  ${d?.fgColor} on ${d?.bgColor}`;
    }),
  );
}

for (const theme of ["light", "dark"] as const) {
  test.describe(`${theme} theme`, () => {
    // the saved preference, the way the app starts when a user last chose it
    test.beforeEach(async ({ page }) => {
      await page.addInitScript((t) => localStorage.setItem("lightsim-theme", t), theme);
    });

    test("topology workspace", async ({ page }) => {
      await openApp(page);
      await check(page, `topology-${theme}`);
    });

    test("results page", async ({ page }) => {
      await openApp(page);
      await runActiveCase(page);
      await expect(drawnLines(page).first()).toBeVisible();
      await check(page, `results-${theme}`);
      // axe leaves text over a chart unjudged: legend names use the text
      // colour, as several series colours are too faint for text (GUI-02)
      const text = await page.locator("#root").evaluate((r) => getComputedStyle(r).color);
      await expect(page.locator(".recharts-legend-item-text > span").first()).toHaveCSS("color", text);
    });

    // UX-09: the Problems list with problems in it, against the workspace's
    // baseline
    test("problems list", async ({ page }) => {
      await openApp(page);
      await page.locator(".react-flow__node", { hasText: "Vehicle Task" }).first().click();
      await page.keyboard.press("Delete");
      await showPanel(page, "Problems");
      await expect(page.getByText(/^How to fix: /).first()).toBeVisible();
      await check(page, `topology-${theme}`);
    });

    test("parameter dialog", async ({ page }) => {
      await openApp(page);
      await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().dblclick();
      await expect(page.getByTitle("Close (Esc)")).toBeVisible();
      await check(page, `parameter-dialog-${theme}`);
    });

    // GUI-02: every panel, menu and dialog, including problem states
    test("colour contrast on every panel, menu and dialog", async ({ page }) => {
      const at: Record<string, string[]> = {};
      const scan = async (where: string) => {
        const failures = await contrastFailures(page);
        if (failures.length) at[where] = failures;
      };
      await openApp(page);
      for (const panel of [
        "Elements",
        "Cases & Parameters",
        "Monitors",
        "Messages",
        "Problems",
        "Layer Configurations",
        "Data Bus Connections",
        "Signal Plot",
      ]) {
        await showPanel(page, panel);
        await scan(panel);
      }
      await showPanel(page, "Topology");
      await selectElement(page, "Vehicle");
      await scan("Properties");
      await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().click({ button: "right" });
      await scan("part menu");
      await page.keyboard.press("Escape");
      await ribbonTab(page, "Home").click();
      await page.getByRole("button", { name: "Open", exact: true }).click();
      await scan("Open menu");
      await page.keyboard.press("Escape");
      // a model with problems: status-bar count, Problems rows, error log lines
      await page.getByRole("button", { name: "New", exact: true }).click();
      await showPanel(page, "Components");
      await page.locator("[data-component-id='motor.emotor']").focus();
      await page.keyboard.press("Enter");
      await runButton(page).click(); // blocked by the checks: logs errors
      await showPanel(page, "Problems");
      await expect(page.getByText(/^\d+ errors?$/)).toBeVisible();
      await scan("Problems with errors");
      await showPanel(page, "Messages");
      await scan("Messages with errors");
      await page.getByRole("button", { name: "New", exact: true }).click();
      await expect(page.getByRole("button", { name: "Cancel", exact: true })).toBeVisible();
      await scan("unsaved-changes dialog");
      expect(at, "text below the WCAG AA contrast minimum: ratio, element, colours").toEqual({});
      // low contrast is fixed, never accepted: no baseline may hold it
      const accepted = Object.values(baseline).flat().filter((k) => k.startsWith("color-contrast"));
      expect(accepted, "color-contrast lines in a11y-baseline.json").toEqual([]);
    });
  });
}
