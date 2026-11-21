// Table data in any layout (backend/app/dataio/layouts.py, ImportDataDialog):
// a matrix copied with its titles and units pasted into the E-Motor's loss
// map opens the preview instead of going into the cells, and Apply stores it
// in one undo step; a three-axis list read from a file sets a new Map
// block's inputs and outputs, with the user's yes.
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
        dynamicPorts?: { id: string; name: string; direction: string; unitGroup?: string }[];
        displayUnits?: Record<string, unknown>;
      }[];
      const el = els.find((e) => e.label === label);
      return el ? { value: el.parameterOverrides[key] ?? null, ports: el.dynamicPorts ?? [], units: el.displayUnits } : null;
    },
    [label, key],
  );
}

test("a loss map pasted with its titles and units goes through the preview, in one undo step", async ({ page }) => {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await openApp(page);
  const win = await openParams(page, "E-Motor");
  const table = win.locator('[data-param="power_loss"]');
  const before = (await draftParam(page, "E-Motor", "power_loss"))?.value;

  // as copied from Excel set to German: a title, names and units in the corner, decimal commas
  await win.evaluate(() =>
    navigator.clipboard.writeText(
      "Loss map, supplier data\nTorque [Nm] \\ Speed [rpm]\t0\t4000\t8000\n0\t0,1\t0,6\t1,2\n150\t1,5\t2,4\t3,9\n",
    ),
  );
  await table.locator(".ss-grid tr").nth(1).locator("td").nth(1).click();
  await win.keyboard.press("Control+v");
  const preview = win.getByRole("dialog", { name: /^Import Power Loss .* from the clipboard$/ });
  await expect(preview).toBeVisible();
  await expect(preview.getByText(/^Not pasted into the cells: the clipboard has titles or units/)).toBeVisible();
  await expect(preview.locator("[data-import-layout]")).toHaveText(
    "Found: A matrix (pivot table) (A2:D4) · read as tab-separated, UTF-8, decimal comma",
  );
  await expect(preview.getByRole("combobox", { name: "What Speed [rpm] is" })).toHaveValue("axis:0");
  await expect(preview.getByRole("combobox", { name: "What Torque [Nm] is" })).toHaveValue("axis:1");
  await expect(preview.getByText("rpm = 1/min")).toBeVisible();
  // the table as it will be stored, in the editor's grid with its plot
  const grid = preview.locator("[data-import-grid]");
  await expect(grid.locator(".ss-grid tr").nth(2)).toContainText("3.9");
  await expect(grid.getByRole("img", { name: /^Table plot: / })).toBeVisible();
  // nothing changed yet
  expect((await draftParam(page, "E-Motor", "power_loss"))?.value).toEqual(before);

  await preview.getByRole("button", { name: "Apply (6 points)" }).click();
  await expect(preview).toBeHidden();
  await expect
    .poll(async () => (await draftParam(page, "E-Motor", "power_loss"))?.value)
    .toEqual({ "0": { "0": 0.1, "150": 1.5 }, "4000": { "0": 0.6, "150": 2.4 }, "8000": { "0": 1.2, "150": 3.9 } });
  // one undo step brings the map back
  await win.locator("h1").click();
  await win.keyboard.press("Control+z");
  await expect.poll(async () => (await draftParam(page, "E-Motor", "power_loss"))?.value).toEqual(before);
});

test("a three-axis list read from a file sets a Map block's inputs and outputs", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Home").click();
  await dragComponent(page, "Map", { x: 260, y: 180 });
  await expect(page.locator(".react-flow__node", { hasText: "Map 1" })).toBeVisible();
  const win = await openParams(page, "Map 1");
  const map = win.locator("[data-map-editor]");

  // a list in shuffled order, with notes above it and a units row
  const rows: string[] = [];
  for (const t of [25, 90]) for (const n of [0, 4000, 8000]) for (const m of [0, 100, 200])
    rows.push(`${n};${t};${m};${(0.1 + n / 8000 + m / 100 + t / 100).toFixed(3).replace(".", ",")};${95 - m / 50}`);
  rows.reverse();
  const csv = `Motor B, measured on rig 2\n\nSpeed;Temperature;Torque;Loss;Efficiency\nrpm;degC;Nm;kW;%\n${rows.join("\n")}\n`;
  await map.getByLabel("Table file to import").setInputFiles({ name: "motor-b.csv", mimeType: "text/csv", buffer: Buffer.from(csv) });
  const preview = win.getByRole("dialog", { name: "Import Map from motor-b.csv" });
  await expect(preview.getByText(/^The Map block has no inputs yet/)).toBeVisible();
  await expect(preview.getByRole("button", { name: /^Apply/ })).toBeDisabled();

  // the user's yes: inputs and outputs from the titles and units
  await preview.getByRole("checkbox").check();
  await expect(preview.getByText("(3 inputs, 2 outputs, named and with units from the titles)", { exact: false })).toBeVisible();
  await expect(preview.getByText("A list, a column per axis", { exact: false })).toBeVisible();
  await preview.getByRole("button", { name: "Apply (18 points)" }).click();
  await expect(preview).toBeHidden();

  await expect(map.getByRole("combobox", { name: "Unit of Temperature" })).toHaveValue("Temperature|°C");
  await expect(map.getByRole("combobox", { name: "Unit of Loss" })).toHaveValue("Power|kW");
  await expect
    .poll(async () => (await draftParam(page, "Map 1", "maps"))?.ports.map((p) => `${p.direction}:${p.id}:${p.unitGroup}`))
    .toEqual([
      "input:speed:Rotational Speed",
      "input:temperature:Temperature",
      "input:torque:Torque",
      "output:loss:Power",
      "output:efficiency:Percent",
    ]);
  const maps = (await draftParam(page, "Map 1", "maps"))!.value as Record<string, Record<string, Record<string, Record<string, number>>>>;
  expect(maps.loss["4000"]["90"]["100"]).toBe(2.5);
  expect(maps.efficiency["0"]["25"]).toEqual({ "0": 95, "100": 93, "200": 91 });
});
