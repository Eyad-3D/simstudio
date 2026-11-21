// The table tools in a part's parameter window: the E-Motor's loss map is
// shaded by value, a selected cell marks its point on the plot and a click
// on the plot selects a cell, a selection multiplied from its right-click
// menu comes back with one Ctrl+Z; and in a Map block of three inputs a row
// inserted from its header's right-click menu goes into every slice.
import { expect, test, type Page } from "@playwright/test";
import { dragComponent, openApp, openParams, ribbonTab } from "./app";

/** A part's parameter in the recovery draft (written shortly after an edit). */
function draftParam(page: Page, label: string, key: string) {
  return page.evaluate(
    ([label, key]) => {
      const draft = JSON.parse(localStorage.getItem("lightsim-draft-v1") ?? "null");
      const els = (draft?.project.systems ?? []).flatMap((s: { elements: unknown[] }) => s.elements) as {
        label: string;
        parameterOverrides: Record<string, unknown>;
      }[];
      return els.find((e) => e.label === label)?.parameterOverrides[key] ?? null;
    },
    [label, key],
  );
}

test("a loss map: shaded, a cell and its plot point, a multiplied selection undone in one step", async ({ page }) => {
  await openApp(page);
  const win = await openParams(page, "E-Motor");
  const table = win.locator('[data-param="power_loss"]');
  const cell = (r: number, c: number) => table.locator(".ss-grid tr").nth(r).locator("td").nth(c);

  // shaded at first (a map), the numbers kept; the switch turns it off and on
  await expect(cell(1, 1)).toHaveAttribute("style", /linear-gradient/);
  const shade = table.getByRole("button", { name: "Shade" });
  await expect(shade).toHaveAttribute("aria-pressed", "true");
  await shade.click();
  await expect(cell(1, 1)).not.toHaveAttribute("style", /linear-gradient/);
  await shade.click();

  // a cell marks its point; a click on the plot selects a point's cell
  await cell(2, 2).click();
  await expect(table.locator("[data-plot-mark]")).toHaveText(/^Marked: /);
  await cell(0, 0).click();
  await expect(table.locator("[data-plot-mark]")).not.toHaveText(/^Marked: /);
  await table.getByRole("img", { name: /^Table plot: / }).click();
  await expect(table.locator("[data-plot-mark]")).toHaveText(/^Marked: /);
  await expect(table.locator("td.ss-active.ss-td-body")).toHaveCount(1);

  // two by two cells multiplied by 2 from the right-click menu, then one Ctrl+Z
  const before = await Promise.all([cell(1, 1), cell(2, 2)].map((c) => c.innerText()));
  await cell(1, 1).click();
  await cell(2, 2).click({ modifiers: ["Shift"] });
  await cell(2, 2).click({ button: "right" });
  await win.getByRole("menuitem", { name: "Multiply by…" }).click();
  await win.getByRole("dialog", { name: "Multiply by…" }).getByRole("textbox").fill("2");
  await win.keyboard.press("Enter");
  await expect(cell(1, 1)).toHaveText(String(Number(before[0]) * 2));
  await expect(cell(2, 2)).toHaveText(String(Number(before[1]) * 2));
  await expect(table.locator("[data-grid-notice]")).toHaveText("Multiplied 4 cells by 2.");
  await win.locator("h1").click();
  await win.keyboard.press("Control+z");
  await expect(cell(1, 1)).toHaveText(before[0]);
  await expect(cell(2, 2)).toHaveText(before[1]);
});

test("a Map block of three inputs: a row inserted from its header goes into every slice", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("lightsim-open-last", "1"));
  await page.goto("/");
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("button", { name: /^Blank project/ }).click();
  await dragComponent(page, "Map", { x: 260, y: 180 });
  const win = await openParams(page, "Map 1");
  const map = win.locator("[data-map-editor]");
  for (let i = 0; i < 3; i++) await map.getByRole("button", { name: "Add input" }).click();
  await map.getByRole("button", { name: "Add output" }).click();
  // a second slice of Input 1 (held above the grid)
  await map.locator(".ss-table-held").getByRole("button", { name: "Add" }).click();
  await win.getByRole("textbox", { name: "New Input 1 breakpoint" }).fill("40");
  await win.keyboard.press("Enter");

  // rows are Input 3: a row inserted below its one breakpoint, in both slices
  await map.locator(".ss-grid tr").nth(1).locator("td").nth(0).click({ button: "right" });
  await win.getByRole("menuitem", { name: "Insert row below" }).click();
  await expect(map.getByRole("status")).toHaveText("Inserted Input 3 1: its points are a copy of the edge's, in every slice.");
  await expect
    .poll(() => draftParam(page, "Map 1", "maps"))
    .toEqual({ output_1: { "0": { "0": { "0": 0, "1": 0 } }, "40": { "0": { "0": 0, "1": 0 } } } });
  // a table keeps one breakpoint on every axis: the only column cannot go
  await map.locator(".ss-grid tr").nth(0).locator("td").nth(1).click({ button: "right" });
  await expect(win.getByRole("menuitem", { name: "Delete column" })).toBeDisabled();
});
