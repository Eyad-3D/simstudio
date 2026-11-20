// A parameter, and a table's axis, shown and typed in a unit the user picks
// in the part's parameter window: the project keeps the library's unit, and
// the choice comes back after a reload.
import { expect, test, type Page } from "@playwright/test";
import { openApp, openParams } from "./app";

/** The E-Motor in the recovery draft (written shortly after an edit). */
function draftMotor(page: Page) {
  return page.evaluate(() => {
    const draft = JSON.parse(localStorage.getItem("lightsim-draft-v1") ?? "null");
    const els = (draft?.project.systems ?? []).flatMap((s: { elements: unknown[] }) => s.elements) as {
      label: string;
      parameterOverrides: Record<string, unknown>;
      displayUnits?: unknown;
    }[];
    return els.find((e) => e.label === "E-Motor") ?? null;
  });
}

test("a number and a table axis in a unit of the user's choice; stored in the library's unit", async ({ page }) => {
  await openApp(page);
  let win = await openParams(page, "E-Motor");

  // the number: shown in rad/s, typed in rad/s, stored in 1/min
  await win.getByRole("button", { name: "1/min, unit of Maximum Speed" }).click();
  const chooser = win.getByRole("group", { name: "Unit of Maximum Speed" });
  await chooser.getByRole("button", { name: "rad/s", exact: true }).first().click();
  await expect(chooser).toHaveCount(0);
  const speed = win.getByRole("spinbutton", { name: /^Maximum Speed/ });
  await speed.fill("1000");
  await expect.poll(async () => (await draftMotor(page))?.parameterOverrides.max_speed_rpm).toBe(9549.29658551);

  // the table: the Speed axis of the drag torque shown in rad/s; a breakpoint typed in it
  const table = win.locator('[data-param="drag_torque"]');
  await table.getByRole("button", { name: "1/min, unit of the Speed axis" }).click();
  await table
    .getByRole("group", { name: "Unit of the Speed axis" })
    .getByRole("button", { name: "rad/s", exact: true })
    .first()
    .click();
  await expect(table.locator(".ss-cell-colhead", { hasText: "Speed (rad/s)" })).toBeVisible();
  const last = table.locator(".ss-grid tr").last().locator("td").first();
  await last.click();
  await win.keyboard.type("2000");
  await win.keyboard.press("Enter");
  await expect
    .poll(async () => Object.keys(((await draftMotor(page))?.parameterOverrides.drag_torque ?? {}) as object))
    .toContain("19098.593171");
  await expect.poll(async () => (await draftMotor(page))?.displayUnits).toEqual({
    max_speed_rpm: { value: "rad/s" },
    drag_torque: { axes: ["rad/s"] },
  });

  // after a reload the same choice, and the same numbers on screen
  await win.close();
  page.on("dialog", (d) => d.accept()); // the browser's leave-page prompt
  await page.reload();
  win = await openParams(page, "E-Motor");
  await expect(win.getByRole("button", { name: "rad/s, unit of Maximum Speed" })).toBeVisible();
  await expect(win.getByRole("spinbutton", { name: /^Maximum Speed/ })).toHaveValue("1000");
  await expect(
    win.locator('[data-param="drag_torque"]').locator(".ss-cell-colhead", { hasText: "Speed (rad/s)" }),
  ).toBeVisible();
  await expect(win.locator('[data-param="drag_torque"]').getByText("2000", { exact: true }).first()).toBeVisible();
});
