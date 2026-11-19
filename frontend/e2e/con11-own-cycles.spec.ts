// CON-11: a drive cycle of one's own, imported from a file, kept in the
// project and listed beside the standard cycles; against time or distance.
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonTab, selectElement } from "./app";

async function importCycle(page: Page, field: string, name: string, csv: string) {
  await page.getByRole("combobox", { name: field }).selectOption({ label: "Import a cycle from a file…" });
  const dialog = page.getByRole("dialog", { name: /^Import a drive cycle/ });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel("Drive cycle file to import").setInputFiles({
    name,
    mimeType: "text/csv",
    buffer: Buffer.from(csv),
  });
  return dialog;
}

test("CON-11: a cycle of one's own imports from a CSV, joins the list and sets the case length", async ({ page }) => {
  await openApp(page);
  await selectElement(page, "Vehicle Task");
  const field = page.getByRole("combobox", { name: "Drive Cycle" });

  // a row that is not a number is named, and nothing is added
  let dialog = await importCycle(page, "Drive Cycle", "bad.csv", "Time [s],Speed [km/h]\n0,0\n10,fast\n20,0\n");
  await expect(dialog.getByText("Row 3: 'fast' in cell B3 (Target Speed) is not a number.")).toBeVisible();
  await expect(dialog.getByRole("button", { name: /^Add to project/ })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(field).toHaveValue("");

  dialog = await importCycle(
    page,
    "Drive Cycle",
    "commute.csv",
    "Time [s],Speed [km/h],Grade [%]\n0,0,0\n10,30,0\n40,40,2\n60,20,2\n70,0,0\n",
  );
  await expect(dialog.getByTestId("cycle-import-figures")).toHaveText(
    "Against time: 70 s · 0.53 km · top 40.0 km/h · with grade",
  );
  const name = dialog.getByLabel("Name in the Drive Cycle lists");
  await expect(name).toHaveValue("commute");
  await name.fill("My commute");
  await dialog.getByRole("button", { name: "Add to project (5 points)" }).click();
  await expect(dialog).toBeHidden();

  // picked for the task, listed under This project, sketched from the project
  await expect(field).toHaveValue("own:my-commute");
  await expect(
    page.getByText("My commute (this project's own, against time): 70 s · 0.53 km · top 40.0 km/h · with grade"),
  ).toBeVisible();
  await expect(page.getByTestId("cycle-source")).toHaveText(
    "Source: commute.csv (kept in this project; not a standard cycle).",
  );
  await expect(field.locator("optgroup[label='This project'] option")).toHaveText([
    "My commute · 70 s · 0.53 km · top 40.0 km/h · with grade",
  ]);
  // the case takes its length, as with a standard cycle
  await ribbonTab(page, "Simulations").click();
  await expect(page.getByTitle("Solver settings for this case")).toHaveText(/^70s/);
});

test("CON-11: a lap against distance, and the project's cycles managed in one place", async ({ page }) => {
  await openApp(page);
  await selectElement(page, "Vehicle Task");
  const field = page.getByRole("combobox", { name: "Drive Cycle" });
  let dialog = await importCycle(page, "Drive Cycle", "commute.csv", "t_s,speed_kmh\n0,0\n20,50\n40,0\n");
  await dialog.getByRole("button", { name: /^Add to project/ }).click();
  await expect(field).toHaveValue("own:commute");

  // a logged lap: distance in km is read as such and kept in m
  dialog = await importCycle(
    page,
    "Drive Cycle",
    "lap.csv",
    "Distance [km];Speed [km/h]\n0;20\n0,2;60\n0,4;40\n0,8;30\n",
  );
  await expect(dialog.getByRole("combobox", { name: "Points against" })).toHaveValue("distance");
  await expect(dialog.getByTestId("cycle-import-figures")).toHaveText("Against distance: 800 m · top 60.0 km/h");
  await dialog.getByRole("button", { name: "Add to project (4 points)" }).click();
  await expect(field).toHaveValue("own:lap");
  await expect(
    page.getByText("Read against the distance the car has driven, whatever the Profile Axis says."),
  ).toBeVisible();
  await expect(
    page.getByRole("img", {
      name: "Speed over distance, lap: 800 m · top 60.0 km/h",
    }),
  ).toBeVisible();

  // the project's cycles: the one in use cannot be removed; the other can
  await field.selectOption({ label: "This project's cycles…" });
  const manage = page.getByRole("dialog", {
    name: "This project's drive cycles",
  });
  await expect(
    manage.getByText("Against distance: 800 m · top 60.0 km/h · from lap.csv · used by 'Vehicle Task'"),
  ).toBeVisible();
  const items = manage.getByRole("listitem");
  await expect(items).toHaveCount(2);
  await expect(items.nth(1).getByRole("button", { name: "Remove" })).toBeDisabled();
  await items.nth(0).getByRole("button", { name: "Remove" }).click();
  await expect(items).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(manage).toBeHidden();
  await expect(field.locator("optgroup[label='This project'] option")).toHaveText(["lap · 800 m · top 60.0 km/h"]);
});
