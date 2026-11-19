// RES-10: a re-run is named by what changed since the previous run of its
// case, compared with it (change and % change beside every summary value and
// on the headline numbers), and lists the model edits behind the change; a
// changed part is one click from the diagram. Names and notes are edited in
// Run info and stored with the run.
import { expect, test, type Page } from "@playwright/test";
import { expectProject, headlineTile, openApp, openSummary, ribbonTab, runActiveCase, selectElement, showPanel } from "./app";
import { importProject } from "./ui-helpers";

const primary = (page: Page) => page.getByRole("combobox", { name: "Primary run" });
const options = (page: Page) => primary(page).locator("option").allTextContents();
/** A summary row's cells: label, this run, baseline, change, % change, unit. */
const row = (page: Page, label: string) =>
  page.getByRole("region", { name: "Summary" }).getByRole("row", { name: new RegExp(`^${label}`) }).getByRole("cell");

test("RES-10: a re-run is named by its change and compared with the run before", async ({ page }) => {
  const n = Date.now() % 100000;
  const id = `e2e-res10-${n}`;
  const name = `E2E compare ${n}`;
  await openApp(page);
  // a project of its own, to read its stored runs back
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 60;
  await importProject(page, { ...example, id, name });
  await expectProject(page, name, { unsaved: true });
  await runActiveCase(page);
  await ribbonTab(page, "Home").click();
  await selectElement(page, "Vehicle");
  const mass = page.locator("tr", { hasText: "Vehicle Mass" }).locator("input");
  await mass.fill("2300");
  await mass.press("Tab");
  await runActiveCase(page);

  // named by the change; the first run keeps its clock time
  expect(await options(page)).toEqual(["City Cycle · Vehicle Mass 2,300 kg", expect.stringMatching(/^City Cycle · \d/)]);
  const baseline = page.getByRole("combobox", { name: "Baseline run" });
  await expect(baseline).toHaveValue("");
  await expect(baseline.locator("option").first()).toHaveText(/^Previous run of this case \(\d/);

  // every number against the baseline: heavier uses more, drives as far
  await expect(headlineTile(page, "Consumption")).toContainText(/\+\d+(\.\d+)? \(\+\d+\.\d %\) vs baseline/);
  await openSummary(page);
  const header = page.getByRole("region", { name: "Summary" }).getByRole("row").first();
  await expect(header).toContainText("Change");
  await expect(header).toContainText("% change");
  await expect(row(page, "Consumption").nth(4)).toHaveText(/^\+\d+(\.\d+)? %$/);
  // the same distance to within the 3 decimals shown (values keep every
  // digit since ENG-16, so a tiny change reads +0.000 rather than ~ 0)
  await expect(row(page, "Distance driven").nth(3)).toHaveText(/^[+−-]?0\.000$/);

  // what changed, and the part it is about one click away
  const changed = page.getByRole("region", { name: "What changed" });
  await expect(changed).toContainText("Vehicle · Vehicle Mass 1,927 → 2,300 kg");
  await changed.getByRole("button", { name: "Vehicle · Vehicle Mass 1,927 → 2,300 kg" }).click();
  await expect(page.getByPlaceholder("Search channels…")).toBeHidden();
  await expect(page.locator(".react-flow__node.selected")).toHaveText(/Vehicle/);
  await showPanel(page, "Properties");
  await expect(page.locator("tr", { hasText: "Vehicle Mass" }).locator("input")).toHaveValue("2300");

  // a name and a note of the user's, stored with the run, by keyboard: Enter
  // saves the name and keeps the focus, Tab goes on to the note
  await ribbonTab(page, "Results").click();
  await page.getByTitle(/^Run info/).click();
  const info = page.getByRole("region", { name: "Run info" });
  const nameBox = info.getByRole("textbox", { name: "Name" });
  await nameBox.fill("Heavier car");
  await page.keyboard.press("Enter");
  await expect(nameBox).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(info.getByRole("textbox", { name: "Note" })).toBeFocused();
  await page.keyboard.type("battery upgrade");
  await page.keyboard.press("Tab");
  await expect(primary(page).locator("option").first()).toHaveText("City Cycle · Heavier car");
  await expect
    .poll(async () => (await (await page.request.get(`/api/projects/${id}/runs`)).json())[0])
    .toMatchObject({ name: "Heavier car", note: "battery upgrade" });
  // the first run, with no name and no note, keeps one Name field when named
  await primary(page).selectOption({ index: 1 });
  await nameBox.fill("Lighter car");
  await page.keyboard.press("Enter");
  await expect(nameBox).toHaveCount(1);
  await expect
    .poll(async () => (await (await page.request.get(`/api/projects/${id}/runs`)).json())[1].name)
    .toBe("Lighter car");
  await page.reload();
  await ribbonTab(page, "Results").click();
  await expect(primary(page).locator("option")).toHaveText(["City Cycle · Heavier car", "City Cycle · Lighter car"]);

  // no baseline: no change columns or change lines
  await page.getByRole("combobox", { name: "Baseline run" }).selectOption("none");
  await openSummary(page);
  await expect(page.getByRole("region", { name: "Summary" }).getByRole("row").first()).not.toContainText("Change");
  await expect(headlineTile(page, "Consumption")).not.toContainText("vs baseline");
  await expect(page.getByRole("region", { name: "What changed" })).toHaveCount(0);
});
