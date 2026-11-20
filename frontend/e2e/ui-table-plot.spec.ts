// The table editor in a part's parameter window: the plot under a table
// follows an edit, and Copy and Paste work through their buttons.
import { expect, test, type Page } from "@playwright/test";
import { openApp, openParams, yRange } from "./app";

/** The E-Motor's drag torque in the recovery draft (written shortly after an edit). */
function draftDragTorque(page: Page) {
  return page.evaluate(() => {
    const draft = JSON.parse(localStorage.getItem("lightsim-draft-v1") ?? "null");
    const els = (draft?.project.systems ?? []).flatMap((s: { elements: unknown[] }) => s.elements) as {
      label: string;
      parameterOverrides: Record<string, unknown>;
    }[];
    return (els.find((e) => e.label === "E-Motor")?.parameterOverrides.drag_torque ?? null) as Record<
      string,
      number
    > | null;
  });
}

const Y = "Drag Torque (unpowered) (N·m)";

test("the plot under a table follows an edit; Copy then Paste through the buttons", async ({ page }) => {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await openApp(page);
  const win = await openParams(page, "E-Motor");
  const table = win.locator('[data-param="drag_torque"]');
  const chart = table.getByRole("img", { name: /^Table plot: / });
  await expect(chart).toBeVisible();
  const [, top] = await yRange(chart, Y);
  expect(top).toBeLessThan(50);

  // an edit: the plot's range takes the new value in, and the selected cell is named under it
  const last = table.locator(".ss-grid tr").last().locator("td").nth(1);
  await last.click();
  await win.keyboard.type("50");
  await win.keyboard.press("Enter");
  await expect.poll(async () => (await yRange(chart, Y))[1]).toBeGreaterThanOrEqual(50);
  await last.click();
  await expect(table.locator("[data-plot-mark]")).toHaveText(/^Marked: Speed .+ 50 N·m$/);

  // Copy with one cell selected takes the whole table, headers and all
  await table.getByRole("button", { name: /^Copy/ }).click();
  await expect(table.getByText("Copied the whole table.")).toBeVisible();
  const copied = await win.evaluate(() => navigator.clipboard.readText());
  expect(copied.split("\n")[0]).toBe(`Speed (1/min)\t${Y}`);
  expect(copied.split("\n").at(-1)).toMatch(/\t50$/);

  // another edit, then Paste puts the copied table back over it
  await last.click();
  await win.keyboard.type("7");
  await win.keyboard.press("Enter");
  await expect.poll(async () => Object.values((await draftDragTorque(page)) ?? {}).at(-1)).toBe(7);
  await table.getByRole("button", { name: /^Paste/ }).click();
  await expect(last).toHaveText("50");
  await expect.poll(async () => Object.values((await draftDragTorque(page)) ?? {}).at(-1)).toBe(50);
  await expect.poll(async () => (await yRange(chart, Y))[1]).toBeGreaterThanOrEqual(50);
});
