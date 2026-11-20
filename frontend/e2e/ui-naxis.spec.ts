// A Map block (docs/spec/tables.md): dropped on a new project, given three
// inputs and two outputs in its parameter window, a value edited on a slice
// of the third input, the plot following it, and the project saved and
// opened again with its map as it was.
import { expect, test, type Page } from "@playwright/test";
import { dragComponent, expectProject, openFromMenu, openParams, ribbonTab, yRange } from "./app";

/** The Map block's ports and tables as saved on the server. */
async function savedMap(page: Page, name: string) {
  const list = await (await page.request.get("/api/projects")).json();
  const saved = list.find((p: { name: string }) => p.name === name);
  const project = await (await page.request.get(`/api/projects/${saved.id}`)).json();
  return project.systems
    .flatMap((s: { elements: unknown[] }) => s.elements)
    .find((e: { componentDefId: string }) => e.componentDefId === "signal.map");
}

test("a Map block: three inputs, two outputs, a value edited on a slice, saved and opened again", async ({ page }) => {
  const name = `E2E map ${Date.now() % 100000}`;
  await page.addInitScript(() => localStorage.setItem("lightsim-open-last", "1"));
  await page.goto("/");
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("button", { name: /^Blank project/ }).click();
  await ribbonTab(page, "Project").click();
  await page.getByText("Project name").locator("xpath=following-sibling::input").fill(name);
  await ribbonTab(page, "Home").click();
  await dragComponent(page, "Map", { x: 260, y: 180 });
  await expect(page.locator(".react-flow__node")).toContainText("Map 1");

  let win = await openParams(page, "Map 1");
  const map = win.locator("[data-map-editor]");
  for (let i = 0; i < 3; i++) await map.getByRole("button", { name: "Add input" }).click();
  await map.getByRole("button", { name: "Add output" }).click();
  await map.getByRole("button", { name: "Add output" }).click();
  await map.getByRole("combobox", { name: "Unit of Input 1" }).selectOption("Temperature|°C");
  await map.getByRole("combobox", { name: "Unit of Output 1" }).selectOption("Power|kW");

  // a second Input 1 breakpoint (held above the grid) copies the first's slice, and is shown
  await map.locator(".ss-table-held").getByRole("button", { name: "Add" }).click();
  await win.getByRole("textbox", { name: "New Input 1 breakpoint" }).fill("40");
  await win.keyboard.press("Enter");
  await expect(map.getByRole("status")).toHaveText("Added Input 1 40 °C: its points are a copy of 0's, in every output.");

  // a value typed on that slice: the plot's range and its marked point follow
  const chart = map.getByRole("img", { name: /^Table plot: / });
  await expect(chart).toBeVisible();
  const body = map.locator(".ss-grid tr").nth(1).locator("td").nth(1);
  await body.click();
  await win.keyboard.type("7.5");
  await win.keyboard.press("Enter");
  await expect.poll(async () => (await yRange(chart, "Output 1 (kW)"))[1]).toBeGreaterThanOrEqual(7.5);
  await body.click();
  await expect(map.locator("[data-plot-mark]")).toHaveText(
    "Marked: Input 1 40 °C, Input 2 0, Input 3 0: Output 1 7.5 kW",
  );

  // saved, another project opened, and this one again: the map as it was
  await win.close();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
  const el = await savedMap(page, name);
  expect(el.dynamicPorts.map((p: { direction: string }) => p.direction)).toEqual([
    "input", "input", "input", "output", "output",
  ]);
  expect(el.parameterOverrides.maps).toEqual({
    output_1: { "0": { "0": { "0": 0 } }, "40": { "0": { "0": 7.5 } } },
    output_2: { "0": { "0": { "0": 0 } }, "40": { "0": { "0": 0 } } },
  });
  await openFromMenu(page, "Battery Electric Car");
  await openFromMenu(page, name);
  win = await openParams(page, "Map 1");
  const again = win.locator("[data-map-editor]");
  await again.locator(".ss-table-held").getByRole("combobox").selectOption("40");
  await expect(again.locator(".ss-grid tr").nth(1).locator("td").nth(1)).toHaveText("7.5");
});
